// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! In-process catch-all DNS responder for AP / captive-portal mode.
//!
//! Answers every A query with the device's AP address (`10.11.12.13`) so a
//! phone's captive-portal detection resolves any probe domain to the device and
//! opens the setup UI. This replaces dnsmasq's wildcard DNS; addressing and DHCP
//! leases are owned by systemd-networkd. It runs only while the setup AP is up.

use std::net::Ipv4Addr;
use std::sync::Mutex;
use std::time::Duration;

use hickory_proto::op::{Message, MessageType, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{RData, Record, RecordType};
use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

const AP_IP: Ipv4Addr = Ipv4Addr::new(10, 11, 12, 13);
const TTL: u32 = 10;

static TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Start the catch-all DNS responder on the AP address. Idempotent: a previous
/// instance is stopped first.
pub async fn start() {
    stop();

    // networkd assigns the AP address asynchronously after `reconfigure`, so the
    // address may not be up the instant we try to bind — retry briefly.
    let mut socket = None;
    for _ in 0..15u8 {
        if let Ok(s) = UdpSocket::bind((AP_IP, 53)).await {
            socket = Some(s);
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let Some(socket) = socket else {
        tracing::error!("captive DNS: could not bind {AP_IP}:53; captive redirect disabled");
        return;
    };
    tracing::info!("captive DNS responder active on {AP_IP}:53");

    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 512];
        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, peer)) => {
                    if let Some(reply) = answer(&buf[..len]) {
                        let _ = socket.send_to(&reply, peer).await;
                    }
                }
                Err(e) => tracing::warn!("captive DNS recv error: {e}"),
            }
        }
    });
    if let Ok(mut guard) = TASK.lock() {
        *guard = Some(handle);
    }
}

/// Stop the responder and free `:53` (so systemd-resolved can reclaim it).
pub fn stop() {
    if let Ok(mut guard) = TASK.lock() {
        if let Some(handle) = guard.take() {
            handle.abort();
        }
    }
}

/// Build a response that points every A query at the AP address. Non-A queries
/// get NODATA so clients fall back to IPv4.
fn answer(query: &[u8]) -> Option<Vec<u8>> {
    let request = Message::from_vec(query).ok()?;

    let mut response = Message::new();
    response.set_id(request.id());
    response.set_message_type(MessageType::Response);
    response.set_op_code(request.op_code());
    response.set_recursion_desired(request.recursion_desired());
    response.set_recursion_available(true);
    response.set_authoritative(true);
    response.set_response_code(ResponseCode::NoError);

    for q in request.queries() {
        response.add_query(q.clone());
        if q.query_type() == RecordType::A {
            response.add_answer(Record::from_rdata(
                q.name().clone(),
                TTL,
                RData::A(A(AP_IP)),
            ));
        }
    }

    response.to_vec().ok()
}
