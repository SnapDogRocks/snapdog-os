//! Optional password authentication for the snapdog-ctrl web interface.
//!
//! When enabled, all `/api/*` routes (except `/api/auth/status` and `/api/auth/login`)
//! require a valid bearer token. Tokens are opaque 32-byte hex strings stored in memory.

use std::collections::HashSet;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::{Mutex, RwLock};

const TOKEN_BYTES: usize = 32;
const FAIL_CLOSED_PASSWORD_HASH: &str = "!ctrl-config-unreadable!";

/// Number of wrong-password attempts allowed before a lockout delay kicks in.
const LOCKOUT_FREE_ATTEMPTS: u32 = 3;
/// Delay applied on the first attempt past the free budget.
const LOCKOUT_BASE_SECS: u64 = 5;
/// Delay never grows past this, no matter how many attempts follow.
const LOCKOUT_MAX_SECS: u64 = 300;

/// Seconds to lock out login after `attempt` total failed attempts (0 = no lockout).
/// Doubles per attempt past the free budget: 5, 10, 20, 40, 80, 160, 300 (capped).
fn backoff_delay_secs(attempt: u32) -> u64 {
    if attempt <= LOCKOUT_FREE_ATTEMPTS {
        return 0;
    }
    let exp = (attempt - LOCKOUT_FREE_ATTEMPTS - 1).min(6);
    LOCKOUT_BASE_SECS
        .saturating_mul(1u64 << exp)
        .min(LOCKOUT_MAX_SECS)
}

/// Shared auth state, passed as axum extension.
#[derive(Clone)]
pub struct AuthState(pub Arc<AuthInner>);

pub struct AuthInner {
    /// bcrypt hash of the password, or `None` if auth is disabled.
    pub password_hash: RwLock<Option<String>>,
    /// Set of valid bearer tokens.
    pub tokens: RwLock<HashSet<String>>,
    /// Count of consecutive failed login attempts (global, not per-client).
    failed_attempts: RwLock<u32>,
    /// When the current lockout (if any) expires.
    locked_until: RwLock<Option<Instant>>,
    /// Serializes password persistence with the matching in-memory transition.
    ///
    /// This is an `Arc` so an owned guard can be transferred to the detached
    /// mutation worker after the request has reached the point of no return.
    password_mutation: Arc<Mutex<()>>,
}

impl AuthState {
    /// Load auth state from persistent config.
    pub async fn load() -> Self {
        let hash = match read_password_hash().await {
            Ok(hash) => hash,
            Err(error) => {
                tracing::error!(
                    error = %error,
                    "ctrl.toml auth state is unreadable; authentication is fail-closed"
                );
                Some(FAIL_CLOSED_PASSWORD_HASH.to_string())
            }
        };
        Self(Arc::new(AuthInner {
            password_hash: RwLock::new(hash),
            tokens: RwLock::new(HashSet::new()),
            failed_attempts: RwLock::new(0),
            locked_until: RwLock::new(None),
            password_mutation: Arc::new(Mutex::new(())),
        }))
    }

    pub async fn is_enabled(&self) -> bool {
        self.0.password_hash.read().await.is_some()
    }

    pub async fn verify_password(&self, password: &str) -> bool {
        let guard = self.0.password_hash.read().await;
        guard
            .as_deref()
            .is_some_and(|hash| bcrypt::verify(password, hash).unwrap_or(false))
    }

    pub async fn create_token(&self) -> String {
        use rand::distr::{Alphanumeric, SampleString};
        let token = Alphanumeric.sample_string(&mut rand::rng(), TOKEN_BYTES * 2);
        self.0.tokens.write().await.insert(token.clone());
        token
    }

    pub async fn revoke_token(&self, token: &str) {
        self.0.tokens.write().await.remove(token);
    }

    pub async fn revoke_all(&self) {
        self.0.tokens.write().await.clear();
    }

    pub async fn is_valid_token(&self, token: &str) -> bool {
        self.0.tokens.read().await.contains(token)
    }

    pub async fn set_password(&self, password: &str) -> anyhow::Result<()> {
        let hash = bcrypt::hash(password, bcrypt::DEFAULT_COST)?;
        let persisted_hash = hash.clone();
        self.dispatch_password_mutation(Some(hash), async move {
            persist_password_hash(Some(&persisted_hash)).await
        })
        .await
    }

    pub async fn remove_password(&self) -> anyhow::Result<()> {
        self.dispatch_password_mutation(None, persist_password_hash(None))
            .await
    }

    /// Persist and apply a password transition as one cancellation-safe unit.
    ///
    /// Waiting for an earlier mutation remains cancellable and has no side
    /// effects. Once this call owns the mutation lock, however, the worker is
    /// detached from the request task. Dropping the request's `JoinHandle`
    /// therefore cannot strand a successfully persisted hash without applying
    /// the same state in memory (or vice versa).
    async fn dispatch_password_mutation<F>(
        &self,
        next_hash: Option<String>,
        persistence: F,
    ) -> anyhow::Result<()>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        // Tokio's mutex is FIFO. Acquire it before spawning so accepted
        // mutations retain request order, and transfer the owned guard to the
        // worker so request cancellation cannot release it prematurely.
        let mutation_guard = self.0.password_mutation.clone().lock_owned().await;
        let auth = self.clone();
        let worker = tokio::spawn(async move {
            let _mutation_guard = mutation_guard;
            persistence.await?;
            auth.apply_password_state(next_hash).await;
            Ok(())
        });

        worker
            .await
            .map_err(|error| anyhow::anyhow!("password mutation worker failed: {error}"))?
    }

    async fn apply_password_state(&self, next_hash: Option<String>) {
        // Take every affected lock before changing anything. The transition
        // itself then contains no await, so it cannot expose a newly applied
        // password together with old bearer tokens or stale lockout metadata.
        let mut password_hash = self.0.password_hash.write().await;
        let mut tokens = self.0.tokens.write().await;
        let mut failed_attempts = self.0.failed_attempts.write().await;
        let mut locked_until = self.0.locked_until.write().await;

        *password_hash = next_hash;
        tokens.clear();
        *failed_attempts = 0;
        *locked_until = None;

        // Keep all four write guards through the transition intentionally,
        // then release them in reverse acquisition order.
        drop(locked_until);
        drop(failed_attempts);
        drop(tokens);
        drop(password_hash);
    }

    /// Seconds remaining in the current login lockout, or `None` if login attempts
    /// are currently allowed.
    pub async fn lockout_remaining(&self) -> Option<u64> {
        let until = (*self.0.locked_until.read().await)?;
        let now = Instant::now();
        if now >= until {
            None
        } else {
            Some((until - now).as_secs().max(1))
        }
    }

    /// Record a wrong-password attempt and, past the free budget, arm/extend the lockout.
    pub async fn record_failed_login(&self) {
        let attempt = {
            let mut attempts = self.0.failed_attempts.write().await;
            *attempts = attempts.saturating_add(1);
            *attempts
        };
        let delay = backoff_delay_secs(attempt);
        if delay > 0 {
            *self.0.locked_until.write().await = Some(Instant::now() + Duration::from_secs(delay));
        }
    }

    /// Clear the failure count after a successful login.
    pub async fn record_successful_login(&self) {
        self.reset_lockout().await;
    }

    async fn reset_lockout(&self) {
        *self.0.failed_attempts.write().await = 0;
        *self.0.locked_until.write().await = None;
    }
}

/// Axum middleware: reject unauthenticated requests when auth is enabled.
pub async fn require_auth_ext(
    auth: AuthState,
    req: Request<Body>,
    next: Next,
) -> Result<Response, StatusCode> {
    // Auth disabled → pass through
    if !auth.is_enabled().await {
        return Ok(next.run(req).await);
    }

    // Public endpoints that don't require auth
    let path = req.uri().path();
    if path == "/api/auth/status" || path == "/api/auth/login" || path == "/api/ws" {
        return Ok(next.run(req).await);
    }

    // Non-API routes (static assets) don't require auth
    if !path.starts_with("/api/") {
        return Ok(next.run(req).await);
    }

    // Extract bearer token
    let token = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) if auth.is_valid_token(t).await => Ok(next.run(req).await),
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

// --- Persistence ---

async fn read_password_hash() -> anyhow::Result<Option<String>> {
    let document = crate::system::read_ctrl_document().await?;
    password_hash_from_document(&document)
}

fn update_password_hash_document(
    document: &mut toml_edit::DocumentMut,
    hash: Option<&str>,
) -> anyhow::Result<()> {
    if let Some(hash) = hash {
        let auth = crate::system::ctrl_table_mut(document, "auth")?;
        auth["password_hash"] = toml_edit::value(hash);
    } else if document.get("auth").is_some() {
        let table = crate::system::ctrl_table_mut(document, "auth")?;
        table.remove("password_hash");
    }
    Ok(())
}

fn password_hash_from_document(
    document: &toml_edit::DocumentMut,
) -> anyhow::Result<Option<String>> {
    crate::system::validate_ctrl_document(document)?;
    let Some(auth) = document.get("auth") else {
        return Ok(None);
    };
    let auth = auth
        .as_table()
        .ok_or_else(|| anyhow::anyhow!("ctrl.toml [auth] must be a table"))?;
    auth.get("password_hash").map_or_else(
        || Ok(None),
        |hash| {
            hash.as_str()
                .map(|hash| Some(hash.to_string()))
                .ok_or_else(|| anyhow::anyhow!("ctrl.toml [auth].password_hash must be a string"))
        },
    )
}

async fn persist_password_hash(hash: Option<&str>) -> anyhow::Result<()> {
    crate::system::update_ctrl_document(|document| update_password_hash_document(document, hash))
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auth_state_with_password(password_hash: Option<&str>) -> AuthState {
        AuthState(Arc::new(AuthInner {
            password_hash: RwLock::new(password_hash.map(str::to_string)),
            tokens: RwLock::new(HashSet::new()),
            failed_attempts: RwLock::new(0),
            locked_until: RwLock::new(None),
            password_mutation: Arc::new(Mutex::new(())),
        }))
    }

    async fn assert_mutation_survives_caller_cancellation(
        initial_hash: Option<&str>,
        next_hash: Option<&str>,
    ) {
        let auth = auth_state_with_password(initial_hash);
        let token = auth.create_token().await;
        *auth.0.failed_attempts.write().await = 7;
        *auth.0.locked_until.write().await = Some(Instant::now() + Duration::from_secs(60));

        // Hold the first in-memory lock so the worker is deterministically
        // suspended after persistence has completed but before it can commit
        // any runtime state: this is the original cancellation window.
        let password_guard = auth.0.password_hash.write().await;
        let (persisted_tx, persisted_rx) = tokio::sync::oneshot::channel();
        let caller_auth = auth.clone();
        let desired_hash = next_hash.map(str::to_string);
        let caller = tokio::spawn(async move {
            caller_auth
                .dispatch_password_mutation(desired_hash, async move {
                    let _ = persisted_tx.send(());
                    Ok(())
                })
                .await
        });

        tokio::time::timeout(Duration::from_secs(1), persisted_rx)
            .await
            .expect("persistence did not complete")
            .expect("persistence worker exited early");
        assert_eq!(password_guard.as_deref(), initial_hash);

        caller.abort();
        assert!(caller.await.unwrap_err().is_cancelled());
        drop(password_guard);

        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let password_applied = auth.0.password_hash.read().await.as_deref() == next_hash;
                let token_revoked = !auth.is_valid_token(&token).await;
                let attempts_reset = *auth.0.failed_attempts.read().await == 0;
                let lockout_reset = auth.0.locked_until.read().await.is_none();
                if password_applied && token_revoked && attempts_reset && lockout_reset {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("detached password mutation did not reconcile runtime state");
    }

    #[test]
    fn free_attempts_have_no_delay() {
        for attempt in 1..=LOCKOUT_FREE_ATTEMPTS {
            assert_eq!(backoff_delay_secs(attempt), 0);
        }
    }

    #[test]
    fn delay_doubles_then_caps() {
        assert_eq!(backoff_delay_secs(4), 5);
        assert_eq!(backoff_delay_secs(5), 10);
        assert_eq!(backoff_delay_secs(6), 20);
        assert_eq!(backoff_delay_secs(7), 40);
        assert_eq!(backoff_delay_secs(8), 80);
        assert_eq!(backoff_delay_secs(9), 160);
        assert_eq!(backoff_delay_secs(10), 300);
        assert_eq!(backoff_delay_secs(50), 300);
    }

    #[tokio::test]
    async fn set_and_remove_password_survive_cancellation_after_persistence() {
        assert_mutation_survives_caller_cancellation(None, Some("new-hash")).await;
        assert_mutation_survives_caller_cancellation(Some("old-hash"), None).await;
    }

    #[tokio::test]
    async fn persistence_failure_leaves_all_runtime_auth_state_unchanged() {
        let auth = auth_state_with_password(Some("old-hash"));
        let token = auth.create_token().await;
        *auth.0.failed_attempts.write().await = 4;
        let locked_until = Instant::now() + Duration::from_secs(60);
        *auth.0.locked_until.write().await = Some(locked_until);

        let error = auth
            .dispatch_password_mutation(Some("new-hash".to_string()), async {
                anyhow::bail!("simulated persistence failure")
            })
            .await
            .unwrap_err();

        assert!(error.to_string().contains("simulated persistence failure"));
        assert_eq!(
            auth.0.password_hash.read().await.as_deref(),
            Some("old-hash")
        );
        assert!(auth.is_valid_token(&token).await);
        assert_eq!(*auth.0.failed_attempts.read().await, 4);
        assert_eq!(*auth.0.locked_until.read().await, Some(locked_until));
    }

    #[test]
    fn auth_document_updates_preserve_service_state() {
        let mut document: toml_edit::DocumentMut = "[services]\nserver = true\n".parse().unwrap();
        update_password_hash_document(&mut document, Some("bcrypt-hash")).unwrap();
        assert_eq!(document["services"]["server"].as_bool(), Some(true));
        assert_eq!(
            password_hash_from_document(&document).unwrap().as_deref(),
            Some("bcrypt-hash")
        );
        update_password_hash_document(&mut document, None).unwrap();
        assert_eq!(document["services"]["server"].as_bool(), Some(true));
        assert_eq!(password_hash_from_document(&document).unwrap(), None);
    }

    #[test]
    fn malformed_auth_shape_and_password_hash_type_fail_closed() {
        for source in ["auth = 'disabled'\n", "[auth]\npassword_hash = 42\n"] {
            let document: toml_edit::DocumentMut = source.parse().unwrap();
            assert!(password_hash_from_document(&document).is_err(), "{source}");
        }
    }

    #[test]
    fn auth_mutations_reject_a_non_table_section_without_panicking() {
        let mut document: toml_edit::DocumentMut = "auth = false\n".parse().unwrap();
        assert!(update_password_hash_document(&mut document, Some("bcrypt-hash")).is_err());
        assert!(update_password_hash_document(&mut document, None).is_err());
    }

    #[test]
    fn unreadable_config_sentinel_cannot_authenticate() {
        assert!(!bcrypt::verify("anything", FAIL_CLOSED_PASSWORD_HASH).unwrap_or(false));
    }
}
