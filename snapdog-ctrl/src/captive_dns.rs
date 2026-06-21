// SPDX-License-Identifier: GPL-3.0-only
// Copyright (C) 2026 Fabian Schmieder

//! In-process catch-all DNS responder for AP / captive-portal mode.
//!
//! Answers every A query with the device's AP address (`network::AP_IP`) so a
//! phone's captive-portal detection resolves any probe domain to the device and
//! opens the setup UI. This replaces dnsmasq's wildcard DNS; addressing and DHCP
//! leases are owned by systemd-networkd. It runs only while the setup AP is up.
//!
//! The responder is hand-rolled (no DNS library): it reads the first question,
//! echoes it, and appends a single compressed A answer. Anything it cannot parse
//! is dropped. That is all a captive portal needs, and it keeps the dependency
//! and CVE surface at zero.

use std::sync::Mutex;
use std::time::Duration;

use tokio::net::UdpSocket;
use tokio::task::JoinHandle;

use crate::network::AP_IP;

/// Short TTL — clients should not cache the captive answer for long.
const TTL_SECS: u32 = 10;
/// DNS header length in bytes.
const HEADER_LEN: usize = 12;

static TASK: Mutex<Option<JoinHandle<()>>> = Mutex::new(None);

/// Start the catch-all DNS responder on the AP address. Idempotent: a previous
/// instance is stopped first.
pub async fn start() {
    stop();

    // networkd assigns the AP address asynchronously after `reconfigure`, so it
    // may not be up the instant we try to bind — retry briefly.
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
                    if let Some(reply) = build_response(&buf[..len]) {
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

/// Build a response pointing every A query at the AP address. Returns `None` for
/// anything that is not a parseable standard query carrying a question.
fn build_response(query: &[u8]) -> Option<Vec<u8>> {
    if query.len() < HEADER_LEN {
        return None;
    }
    if u16::from_be_bytes([query[4], query[5]]) == 0 {
        return None; // no question
    }

    // Walk the first question's QNAME (length-prefixed labels until a zero byte).
    let mut pos = HEADER_LEN;
    loop {
        let len = usize::from(*query.get(pos)?);
        if len == 0 {
            pos += 1;
            break;
        }
        if len >= 0xC0 {
            return None; // compression pointer in the question — not handled
        }
        pos += 1 + len;
    }
    let qtype = u16::from_be_bytes([*query.get(pos)?, *query.get(pos + 1)?]);
    let question_end = pos + 4; // QTYPE (2) + QCLASS (2)
    if query.len() < question_end {
        return None;
    }

    let is_a = qtype == 1; // A record
    let mut resp = Vec::with_capacity(question_end + 16);

    // Header.
    resp.extend_from_slice(&query[0..2]); // echo transaction ID
    resp.push(0x80 | (query[2] & 0x79) | 0x04); // QR=1, opcode + RD echoed, AA=1
    resp.push(0x80); // RA=1, RCODE=NoError
    resp.extend_from_slice(&1u16.to_be_bytes()); // QDCOUNT
    resp.extend_from_slice(&u16::from(is_a).to_be_bytes()); // ANCOUNT
    resp.extend_from_slice(&0u16.to_be_bytes()); // NSCOUNT
    resp.extend_from_slice(&0u16.to_be_bytes()); // ARCOUNT

    // Question, echoed verbatim.
    resp.extend_from_slice(&query[HEADER_LEN..question_end]);

    // One A answer for every name (NODATA for non-A, so clients fall back).
    if is_a {
        resp.extend_from_slice(&[0xC0, 0x0C]); // NAME = pointer to the question
        resp.extend_from_slice(&1u16.to_be_bytes()); // TYPE = A
        resp.extend_from_slice(&1u16.to_be_bytes()); // CLASS = IN
        resp.extend_from_slice(&TTL_SECS.to_be_bytes());
        resp.extend_from_slice(&4u16.to_be_bytes()); // RDLENGTH
        resp.extend_from_slice(&AP_IP.octets());
    }

    Some(resp)
}
