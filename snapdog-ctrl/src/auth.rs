//! Optional password authentication for the snapdog-ctrl web interface.
//!
//! When enabled, all `/api/*` routes (except `/api/auth/status` and `/api/auth/login`)
//! require a valid bearer token. Tokens are opaque 32-byte hex strings stored in memory.

use std::collections::HashSet;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::middleware::Next;
use axum::response::Response;
use tokio::sync::RwLock;

const CTRL_CONFIG_PATH: &str = "/data/snapdog/ctrl.toml";
const TOKEN_BYTES: usize = 32;

/// Shared auth state, passed as axum extension.
#[derive(Clone)]
pub struct AuthState(pub Arc<AuthInner>);

pub struct AuthInner {
    /// bcrypt hash of the password, or `None` if auth is disabled.
    pub password_hash: RwLock<Option<String>>,
    /// Set of valid bearer tokens.
    pub tokens: RwLock<HashSet<String>>,
}

impl AuthState {
    /// Load auth state from persistent config.
    pub async fn load() -> Self {
        let hash = read_password_hash().await;
        Self(Arc::new(AuthInner {
            password_hash: RwLock::new(hash),
            tokens: RwLock::new(HashSet::new()),
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
        *self.0.password_hash.write().await = Some(hash.clone());
        persist_password_hash(Some(&hash)).await
    }

    pub async fn remove_password(&self) -> anyhow::Result<()> {
        *self.0.password_hash.write().await = None;
        self.revoke_all().await;
        persist_password_hash(None).await
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

async fn read_password_hash() -> Option<String> {
    let content = tokio::fs::read_to_string(CTRL_CONFIG_PATH).await.ok()?;
    let doc: toml_edit::DocumentMut = content.parse().ok()?;
    doc.get("auth")?
        .get("password_hash")?
        .as_str()
        .map(String::from)
}

async fn persist_password_hash(hash: Option<&str>) -> anyhow::Result<()> {
    let content = tokio::fs::read_to_string(CTRL_CONFIG_PATH)
        .await
        .unwrap_or_default();
    let mut doc: toml_edit::DocumentMut = content.parse().unwrap_or_default();

    if let Some(h) = hash {
        let auth = doc
            .entry("auth")
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
        auth["password_hash"] = toml_edit::value(h);
    } else if let Some(auth) = doc.get_mut("auth")
        && let Some(tbl) = auth.as_table_mut()
    {
        tbl.remove("password_hash");
    }

    // Ensure parent directory exists
    if let Some(parent) = std::path::Path::new(CTRL_CONFIG_PATH).parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    crate::system::atomic_write(CTRL_CONFIG_PATH, &doc.to_string()).await?;
    Ok(())
}
