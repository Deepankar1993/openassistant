// src/gateway/slack.rs
//! Slack gateway via the Events API. Mounted at `POST /slack/events` on the
//! WebChat axum server (see `webchat::build_router`). Verifies the Slack request
//! signature, answers the url_verification handshake, and routes message events
//! through the agent, replying via `chat.postMessage`.
//!
//! Operational requirement: Slack must be able to reach this endpoint, so the
//! WebChat server needs a public URL (e.g. a tunnel/reverse proxy). Configure
//! `gateway.slack_token` (bot token) and `gateway.slack_signing_secret`.

use anyhow::Result;
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use tracing::{debug, error, warn};

use super::session_store::ChannelSessionStore;
use super::webchat::{Convo, GatewayState};

type HmacSha256 = Hmac<Sha256>;

/// Reject requests whose timestamp is older than this (replay protection).
const MAX_SKEW_SECS: i64 = 60 * 5;

/// Bound persisted session growth (Slack channels are long-lived).
const MAX_SESSION_MESSAGES: usize = 40;

pub async fn events_handler(
    State(state): State<GatewayState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let secret = state.config.gateway.slack_signing_secret.clone();
    if !secret.is_empty() && !verify_signature(&headers, &body, &secret) {
        warn!("Slack request failed signature verification");
        return (StatusCode::UNAUTHORIZED, "invalid signature").into_response();
    }

    let payload: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, format!("bad json: {e}")).into_response(),
    };

    // 1) URL verification handshake — echo the challenge.
    if payload["type"] == "url_verification" {
        let challenge = payload["challenge"].as_str().unwrap_or("").to_string();
        return (StatusCode::OK, challenge).into_response();
    }

    // 2) Event callback — handle user message events.
    if payload["type"] == "event_callback" {
        let event = &payload["event"];
        let is_message = event["type"] == "message";
        // Ignore bot messages / our own echoes / message subtypes (edits, joins…).
        let is_from_user = event["bot_id"].is_null() && event["subtype"].is_null();
        if is_message && is_from_user {
            let text = event["text"].as_str().unwrap_or("").trim().to_string();
            let channel = event["channel"].as_str().unwrap_or("").to_string();
            let user = event["user"].as_str().unwrap_or("unknown").to_string();
            if !text.is_empty() && !channel.is_empty() {
                // Respond out-of-band: Slack requires a 200 within 3 seconds.
                let state = state.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_message(state, channel, user, text).await {
                        error!("Slack event handling failed: {}", e);
                    }
                });
            }
        }
    }

    StatusCode::OK.into_response()
}

async fn handle_message(state: GatewayState, channel: String, user: String, text: String) -> Result<()> {
    let data_dir = state.config.general.data_dir.clone();
    // Opened per call: events may be handled concurrently, so each gets its own
    // connection (best-effort — a failed open degrades to in-memory only).
    let store = ChannelSessionStore::open_default(&data_dir).ok();

    // Serialize this channel's whole turn. Without it, two concurrent events for
    // the same channel both load → process → save and the last writer silently
    // clobbers the other's turn (in memory AND on disk). Cross-channel events
    // still run concurrently.
    let chan_lock = {
        let mut locks = state.slack_locks.lock().await;
        locks
            .entry(channel.clone())
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    };
    let _turn = chan_lock.lock().await;

    // Take this channel's conversation OUT of the map, drop the guard before the
    // agent await, then re-insert — never hold the map lock across `process()`.
    // On a cache miss, restore the persisted session before starting fresh.
    let mut convo = {
        let mut map = state.slack_sessions.lock().await;
        match map.remove(&channel) {
            Some(c) => c,
            None => {
                let mut c = Convo::new("slack", &user, &data_dir);
                if let Some(session) =
                    store.as_ref().and_then(|s| s.load("slack", &channel).ok().flatten())
                {
                    c.session = session;
                }
                c
            }
        }
    };

    let reply = match state.agent.process(&text, &mut convo.ctx, &mut convo.session).await {
        Ok(r) => r,
        Err(e) => format!("⚠️ {}", e),
    };

    // Bound session growth, then persist so it survives a restart.
    let len = convo.session.messages.len();
    if len > MAX_SESSION_MESSAGES {
        convo.session.messages.drain(0..(len - MAX_SESSION_MESSAGES));
    }
    if let Some(store) = store.as_ref() {
        if let Err(e) = store.save("slack", &channel, &convo.session) {
            warn!("Slack: could not persist session for {}: {}", channel, e);
        }
    }

    {
        let mut map = state.slack_sessions.lock().await;
        map.insert(channel.clone(), convo);
    }

    post_message(&state.config.gateway.slack_token, &channel, &reply).await
}

async fn post_message(token: &str, channel: &str, text: &str) -> Result<()> {
    if token.is_empty() {
        anyhow::bail!("no slack_token configured — cannot reply");
    }
    let client = reqwest::Client::new();
    let resp = client
        .post("https://slack.com/api/chat.postMessage")
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({ "channel": channel, "text": text }))
        .send()
        .await?;
    let json: serde_json::Value = resp.json().await.unwrap_or_default();
    if json["ok"] != true {
        anyhow::bail!("chat.postMessage error: {}", json["error"].as_str().unwrap_or("unknown"));
    }
    Ok(())
}

/// Verify `X-Slack-Signature` per Slack's spec:
/// `v0=HMAC_SHA256(signing_secret, "v0:{timestamp}:{body}")`, with a freshness
/// window on the timestamp to blunt replay attacks.
fn verify_signature(headers: &HeaderMap, body: &[u8], secret: &str) -> bool {
    let Some(ts) = header(headers, "x-slack-request-timestamp") else { return false };
    let Some(sig) = header(headers, "x-slack-signature") else { return false };

    if let Ok(ts_num) = ts.parse::<i64>() {
        let now = chrono::Utc::now().timestamp();
        if (now - ts_num).abs() > MAX_SKEW_SECS {
            debug!("Slack timestamp outside freshness window");
            return false;
        }
    } else {
        return false;
    }

    let basestring = format!("v0:{}:{}", ts, String::from_utf8_lossy(body));
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else { return false };
    mac.update(basestring.as_bytes());
    let expected = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

    // Length-then-byte compare; the MAC itself is the security boundary.
    expected.len() == sig.len() && expected.as_bytes().iter().zip(sig.as_bytes()).fold(0u8, |a, (x, y)| a | (x ^ y)) == 0
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers.get(name).and_then(|v| v.to_str().ok())
}

/// Start placeholder retained for API symmetry; Slack runs inside the WebChat
/// server (see `webchat::build_router`), not as a standalone task.
pub async fn start(_token: &str) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signature_roundtrips() {
        let secret = "8f742231b10e8888abcd99yyyzzz85a5";
        let ts = chrono::Utc::now().timestamp().to_string();
        let body = br#"{"type":"event_callback"}"#;
        let basestring = format!("v0:{}:{}", ts, String::from_utf8_lossy(body));
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(basestring.as_bytes());
        let sig = format!("v0={}", hex::encode(mac.finalize().into_bytes()));

        let mut headers = HeaderMap::new();
        headers.insert("x-slack-request-timestamp", ts.parse().unwrap());
        headers.insert("x-slack-signature", sig.parse().unwrap());
        assert!(verify_signature(&headers, body, secret));

        // Tampered body fails.
        assert!(!verify_signature(&headers, b"{}", secret));
    }
}
