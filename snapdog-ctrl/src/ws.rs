// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

use axum::{
    Extension,
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::broadcast;

#[derive(Clone)]
pub struct WsSender(pub broadcast::Sender<String>);

/// Handler for WebSocket upgrades
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Extension(WsSender(tx)): Extension<WsSender>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, tx))
}

async fn handle_socket(socket: WebSocket, tx: broadcast::Sender<String>) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = tx.subscribe();

    // Spawn a task to send broadcast events to this client
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Axum 0.8 `Message::Text` might take an `axum::extract::ws::Utf8Bytes` or similar in Axum 0.8.
            // Let's check Axum 0.8 docs: Message::Text(String) is standard, or Message::Text(Utf8Bytes).
            // Usually Message::Text(msg.into()) or Message::text(msg) works.
            // Let's write `Message::Text(msg.into())` or just use the String directly.
            // In Axum 0.8, `Message::Text(axum::extract::ws::Utf8Bytes::from(msg))` or similar is used,
            // or simply `Message::Text(msg.into())` works because `Utf8Bytes` implements `From<String>`.
            if sender.send(Message::Text(msg.into())).await.is_err() {
                // Connection closed or error
                break;
            }
        }
    });

    // Spawn a task to read from the socket (and handle pings/pongs/closing)
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            match msg {
                Message::Close(_) => break,
                Message::Ping(_p) => {
                    // Axum automatically responds to pings, but we can log or process them if needed.
                }
                _ => {}
            }
        }
    });

    // Wait for either task to complete, then clean up
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    };
}
