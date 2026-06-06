// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use snapdog_update::client::UpdateClient;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

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

    let mut client = UpdateClient::new(&mock_server.uri());
    let res = client.preflight_auth(None).await;
    assert!(res.is_ok());
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

    let client = UpdateClient::new(&mock_server.uri());
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

    let client = UpdateClient::new(&mock_server.uri());
    let health = client.system_health().await.unwrap();
    assert!(health.ok);
    assert!(health.warnings.is_empty());
}
