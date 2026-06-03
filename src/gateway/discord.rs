// src/gateway/discord.rs
//! Discord gateway — connects via `serenity` and replies to allowed users by
//! running their message through the agent core (`Agent::process`).
//!
//! Concurrency: each user has an isolated `Session` + `FullContext` kept in an
//! async map. The lock guard is taken/dropped AROUND `Agent::process().await`
//! (never held across it), so one user's turn never blocks another's — and so
//! the code compiles (a `std::sync::Mutex` guard is `!Send` across an await).

use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use serenity::all::{Client, Context as SerenityContext, EventHandler, GatewayIntents, Message as DiscordMessage, Ready};
use serenity::async_trait;

use crate::config::Config;
use crate::core::agent::Agent;
use crate::core::persona::{FullContext, Persona};
use crate::core::session::Session;

/// Trim in-memory sessions to this many messages. The bot is long-running
/// (unlike the one-shot CLI), so `Session::messages` would otherwise grow
/// unbounded. `call_llm` already trims the HTTP body to the last 30.
const MAX_SESSION_MESSAGES: usize = 40;
/// Discord's hard message cap is 2000 chars; leave headroom for safety.
const DISCORD_MAX_LEN: usize = 1900;

/// Per-user conversation state.
struct UserState {
    ctx: FullContext,
    session: Session,
}

struct Handler {
    agent: Arc<Agent>,
    sessions: Arc<Mutex<HashMap<u64, UserState>>>,
    allowed_users: Vec<String>,
    dm_policy: String,
    data_dir: String,
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: SerenityContext, ready: Ready) {
        info!("Discord connected as {}", ready.user.name);
        info!(
            "Note: the MESSAGE_CONTENT privileged intent must be enabled in the Discord \
             Developer Portal, or incoming message text will be empty."
        );
    }

    async fn message(&self, ctx: SerenityContext, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }
        let user_id = msg.author.id.get();
        if !gate(&user_id.to_string(), &self.allowed_users, &self.dm_policy) {
            return;
        }
        let content = msg.content.trim().to_string();
        if content.is_empty() {
            return;
        }

        // Take this user's state OUT of the map and drop the guard before the
        // await — never hold the lock across Agent::process().
        let mut state = {
            let mut map = self.sessions.lock().await;
            map.remove(&user_id).unwrap_or_else(|| {
                let mut ctx = FullContext::new();
                ctx.persona = Persona::load_or_default(&self.data_dir);
                UserState {
                    ctx,
                    session: Session::new("discord", user_id.to_string()),
                }
            })
        };

        let reply = match self.agent.process(&content, &mut state.ctx, &mut state.session).await {
            Ok(r) => r,
            Err(e) => format!("⚠️ {}", e),
        };

        // Bound session growth.
        let len = state.session.messages.len();
        if len > MAX_SESSION_MESSAGES {
            state.session.messages.drain(0..(len - MAX_SESSION_MESSAGES));
        }

        // Write the state back.
        {
            let mut map = self.sessions.lock().await;
            map.insert(user_id, state);
        }

        for chunk in chunk_message(&reply) {
            if let Err(e) = msg.channel_id.say(&ctx.http, chunk).await {
                error!("Discord send failed: {}", e);
                break;
            }
        }
    }
}

/// Start the Discord gateway. Blocks until the client disconnects.
pub async fn start(config: Config) -> Result<()> {
    let token = config.gateway.discord_token.clone();
    if token.trim().is_empty() {
        anyhow::bail!("No Discord token configured (gateway.discord_token).");
    }
    if config.gateway.discord_allowed_users.is_empty() && config.gateway.dm_policy != "open" {
        warn!(
            "No gateway.discord_allowed_users set and dm_policy is not 'open' — the bot will \
             ignore everyone. Set an allowlist or dm_policy=open."
        );
    }

    let agent = Agent::new(config.model.model.clone())
        .with_workspace(config.general.data_dir.clone())
        .with_tools_enabled(config.tools.enabled);

    let handler = Handler {
        agent: Arc::new(agent),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        allowed_users: config.gateway.discord_allowed_users.clone(),
        dm_policy: if config.gateway.dm_policy.is_empty() {
            "pairing".to_string()
        } else {
            config.gateway.dm_policy.clone()
        },
        data_dir: config.general.data_dir.clone(),
    };

    let intents =
        GatewayIntents::GUILD_MESSAGES | GatewayIntents::DIRECT_MESSAGES | GatewayIntents::MESSAGE_CONTENT;

    info!("Starting Discord client…");
    let mut client = Client::builder(&token, intents).event_handler(handler).await?;
    client.start().await?;
    Ok(())
}

/// Whether a user may talk to the bot. An explicit allowlist is authoritative;
/// with no allowlist, only `dm_policy = "open"` admits everyone.
fn gate(user_id: &str, allowed: &[String], dm_policy: &str) -> bool {
    if !allowed.is_empty() {
        return is_allowed(user_id, allowed);
    }
    dm_policy == "open"
}

pub fn is_allowed(user_id: &str, allowed: &[String]) -> bool {
    if allowed.is_empty() {
        return true;
    }
    allowed.iter().any(|id| id == user_id || id == "*")
}

/// Split a reply into Discord-sized chunks, preferring line boundaries.
fn chunk_message(s: &str) -> Vec<String> {
    if s.trim().is_empty() {
        return vec!["(empty response)".to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for line in s.split_inclusive('\n') {
        if cur.len() + line.len() > DISCORD_MAX_LEN && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if line.len() > DISCORD_MAX_LEN {
            for ch in line.chars() {
                if cur.len() + ch.len_utf8() > DISCORD_MAX_LEN {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push(ch);
            }
        } else {
            cur.push_str(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_allows_listed_users_only() {
        let allowed = vec!["123".to_string()];
        assert!(gate("123", &allowed, "pairing"));
        assert!(!gate("999", &allowed, "pairing"));
    }

    #[test]
    fn gate_empty_allowlist_respects_policy() {
        assert!(gate("123", &[], "open"));
        assert!(!gate("123", &[], "pairing"));
    }

    #[test]
    fn chunking_splits_long_messages() {
        let big = "x".repeat(5000);
        let chunks = chunk_message(&big);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.len() <= DISCORD_MAX_LEN));
    }
}
