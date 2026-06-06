// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use snapdog_update::client::UpdateClient;
use snapdog_update::error::UpgradeError;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[test]
fn test_update_client_rejects_unsupported_url_scheme() {
    let result = UpdateClient::new("ftp://snapdog.local");
    assert!(matches!(result, Err(UpgradeError::InvalidBaseUrl(_))));
}

#[tokio::test]
async fn test_preflight_auth_disabled() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/auth/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "enabled": false,
            "authenticated": true
        })))
        .mount(&mock_server)
        .await;

    let mut client = UpdateClient::new(&mock_server.uri()).unwrap();
    let res = client.preflight_auth(None, false).await;
    assert!(res.is_ok());
}

#[tokio::test]
async fn test_preflight_auth_non_interactive_requires_password() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/auth/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "enabled": true,
            "authenticated": false
        })))
        .mount(&mock_server)
        .await;

    let mut client = UpdateClient::new(&mock_server.uri()).unwrap();
    let err = client.preflight_auth(None, false).await.unwrap_err();
    assert!(matches!(
        err,
        UpgradeError::NonInteractiveInputRequired {
            input: "password",
            ..
        }
    ));
}

#[tokio::test]
async fn test_preflight_auth_login_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/auth/status"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "enabled": true,
            "authenticated": false
        })))
        .mount(&mock_server)
        .await;

    Mock::given(method("POST"))
        .and(path("/api/auth/login"))
        .and(body_json(serde_json::json!({ "password": "secret" })))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "token": "test-token"
        })))
        .mount(&mock_server)
        .await;

    let mut client = UpdateClient::new(&mock_server.uri()).unwrap();
    client.preflight_auth(Some("secret"), false).await.unwrap();
}

#[tokio::test]
async fn test_system_info_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/system"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "hostname": "test-host",
            "version": "1.2.3",
            "board_model": "Raspberry Pi 4 Model B",
            "uptime_seconds": 3600
        })))
        .mount(&mock_server)
        .await;

    let client = UpdateClient::new(&mock_server.uri()).unwrap();
    let info = client.system_info().await.unwrap();
    assert_eq!(info.hostname, "test-host");
    assert_eq!(info.version, "1.2.3");
    assert_eq!(info.board_model, "Raspberry Pi 4 Model B");
    assert_eq!(info.uptime_seconds, 3600);
}

#[tokio::test]
async fn test_system_health_success() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/system/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "warnings": []
        })))
        .mount(&mock_server)
        .await;

    let client = UpdateClient::new(&mock_server.uri()).unwrap();
    let health = client.system_health().await.unwrap();
    assert!(health.ok);
    assert!(health.warnings.is_empty());
}

#[tokio::test]
async fn test_system_health_error_preserves_response_body() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/system/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("device busy"))
        .mount(&mock_server)
        .await;

    let client = UpdateClient::new(&mock_server.uri()).unwrap();
    let result = client.system_health().await;
    let Err(err) = result else {
        panic!("503 health response should be an error");
    };
    assert!(matches!(
        err,
        UpgradeError::HttpStatus {
            status,
            body,
            ..
        } if status.as_u16() == 503 && body == "device busy"
    ));
}
