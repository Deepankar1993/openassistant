// src/gateway/discord.rs
//! Discord gateway (Hermes-style) via `serenity`.
//!
//! Behavior:
//! - **@mention** the bot in a channel (or post in the configured **home**
//!   channel) → the bot reacts ✅ and spawns a **thread** off that message,
//!   then replies inside the thread.
//! - Messages **inside a bot-created thread** continue that thread's
//!   conversation — no mention needed.
//! - **DMs** are answered directly (threads aren't available in DMs).
//! - Text **commands** (allowed users only): `set home` / `!home`,
//!   `unset home`, `!new` (reset this conversation), `!help`.
//!
//! Each thread / DM / home conversation is an isolated `Session`. The lock guard
//! is dropped before `Agent::process().await` (never held across it).

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use serenity::all::{
    ChannelId, Client, Context as SerenityContext, CreateThread, EventHandler, GatewayIntents,
    Message as DiscordMessage, ReactionType, Ready,
};
use serenity::async_trait;

use crate::config::Config;
use crate::core::agent::Agent;
use crate::core::persona::{FullContext, Persona};
use crate::core::session::Session;

const MAX_SESSION_MESSAGES: usize = 40;
const DISCORD_MAX_LEN: usize = 1900;
const THREAD_NAME_MAX: usize = 90;

struct Convo {
    ctx: FullContext,
    session: Session,
}

struct Handler {
    agent: Arc<Agent>,
    /// Conversations keyed by channel/thread id.
    sessions: Arc<Mutex<HashMap<u64, Convo>>>,
    /// Thread ids the bot created (continue without a mention).
    threads: Arc<Mutex<HashSet<u64>>>,
    /// Optional home channel id (top-level messages here auto-thread).
    home_channel: Arc<Mutex<Option<u64>>>,
    allowed_users: Vec<String>,
    dm_policy: String,
    data_dir: String,
}

impl Handler {
    fn new_convo(&self, key: &str) -> Convo {
        let mut ctx = FullContext::new();
        ctx.persona = Persona::load_or_default(&self.data_dir);
        Convo { ctx, session: Session::new("discord", key) }
    }

    /// Run text through the agent for a given conversation key. Locks only to
    /// take/return the conversation — never across the agent await.
    async fn respond(&self, key: u64, text: &str) -> String {
        let mut convo = {
            let mut map = self.sessions.lock().await;
            map.remove(&key).unwrap_or_else(|| self.new_convo(&key.to_string()))
        };
        let reply = match self.agent.process(text, &mut convo.ctx, &mut convo.session).await {
            Ok(r) => r,
            Err(e) => format!("⚠️ {}", e),
        };
        let len = convo.session.messages.len();
        if len > MAX_SESSION_MESSAGES {
            convo.session.messages.drain(0..(len - MAX_SESSION_MESSAGES));
        }
        self.sessions.lock().await.insert(key, convo);
        reply
    }

    async fn react_ack(&self, ctx: &SerenityContext, msg: &DiscordMessage) {
        let _ = msg.react(&ctx.http, ReactionType::Unicode("✅".to_string())).await;
    }

    async fn send_chunked(&self, ctx: &SerenityContext, channel: ChannelId, text: &str) {
        for chunk in chunk_message(text) {
            if let Err(e) = channel.say(&ctx.http, chunk).await {
                error!("Discord send failed: {}", e);
                break;
            }
        }
    }

    /// Handle a text command. Returns true if the message was a command.
    async fn try_command(&self, ctx: &SerenityContext, msg: &DiscordMessage, content: &str) -> bool {
        let norm = content.trim().to_lowercase();
        let channel = msg.channel_id;
        match norm.as_str() {
            "set home" | "!home" | "!sethome" => {
                *self.home_channel.lock().await = Some(channel.get());
                // Persist so it survives restarts.
                if let Ok(mut cfg) = crate::config::load().await {
                    cfg.gateway.discord_home_channel = channel.get().to_string();
                    let _ = crate::config::save(&cfg).await;
                }
                self.send_chunked(ctx, channel, "🏠 Home channel set. I'll start a thread for new messages here.").await;
                true
            }
            "unset home" | "!unsethome" => {
                *self.home_channel.lock().await = None;
                if let Ok(mut cfg) = crate::config::load().await {
                    cfg.gateway.discord_home_channel = String::new();
                    let _ = crate::config::save(&cfg).await;
                }
                self.send_chunked(ctx, channel, "🏠 Home channel cleared.").await;
                true
            }
            "!new" | "!reset" => {
                self.sessions.lock().await.remove(&channel.get());
                self.send_chunked(ctx, channel, "🧹 Started a fresh conversation here.").await;
                true
            }
            "!help" => {
                self.send_chunked(ctx, channel, HELP_TEXT).await;
                true
            }
            _ => false,
        }
    }
}

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: SerenityContext, ready: Ready) {
        info!("Discord connected as {}", ready.user.name);
        info!(
            "Note: enable the MESSAGE CONTENT intent in the Developer Portal, and grant the bot \
             Create Public Threads + Send Messages in Threads + Add Reactions, or thread mode won't work."
        );
    }

    async fn message(&self, ctx: SerenityContext, msg: DiscordMessage) {
        if msg.author.bot {
            return;
        }
        if !gate(&msg.author.id.get().to_string(), &self.allowed_users, &self.dm_policy) {
            return;
        }
        let content = msg.content.trim().to_string();
        if content.is_empty() {
            return;
        }

        // Commands first.
        if self.try_command(&ctx, &msg, &content).await {
            return;
        }

        let channel_id = msg.channel_id.get();
        let is_dm = msg.guild_id.is_none();

        // 1) DM → answer directly.
        if is_dm {
            self.react_ack(&ctx, &msg).await;
            let reply = self.respond(channel_id, &content).await;
            self.send_chunked(&ctx, msg.channel_id, &reply).await;
            return;
        }

        // 2) Message inside a bot-created thread → continue it.
        if self.threads.lock().await.contains(&channel_id) {
            self.react_ack(&ctx, &msg).await;
            let reply = self.respond(channel_id, &content).await;
            self.send_chunked(&ctx, msg.channel_id, &reply).await;
            return;
        }

        // 3) Mention OR home channel → spawn a thread and reply inside it.
        let mentioned = msg.mentions_me(&ctx.http).await.unwrap_or(false);
        let in_home = *self.home_channel.lock().await == Some(channel_id);
        if mentioned || in_home {
            self.react_ack(&ctx, &msg).await;
            let title = thread_title(&content);
            match msg
                .channel_id
                .create_thread_from_message(&ctx.http, msg.id, CreateThread::new(title))
                .await
            {
                Ok(thread) => {
                    let tid = thread.id.get();
                    self.threads.lock().await.insert(tid);
                    let reply = self.respond(tid, &content).await;
                    self.send_chunked(&ctx, thread.id, &reply).await;
                }
                Err(e) => {
                    warn!("Could not create thread ({}); replying in channel.", e);
                    let reply = self.respond(channel_id, &content).await;
                    self.send_chunked(&ctx, msg.channel_id, &reply).await;
                }
            }
        }
        // Otherwise: not addressed to the bot — ignore.
    }
}

const HELP_TEXT: &str = "🦉 openAssistant — Discord commands:\n\
• @mention me in a channel → I'll spawn a thread and we continue there\n\
• `set home` / `!home` → make this channel home (new messages here auto-thread)\n\
• `unset home` → clear the home channel\n\
• `!new` → start a fresh conversation in this thread/DM\n\
• `!help` → this message\n\
DM me directly for a 1:1 chat (no thread).";

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

    let home = config.gateway.discord_home_channel.trim().parse::<u64>().ok();

    let handler = Handler {
        agent: Arc::new(agent),
        sessions: Arc::new(Mutex::new(HashMap::new())),
        threads: Arc::new(Mutex::new(HashSet::new())),
        home_channel: Arc::new(Mutex::new(home)),
        allowed_users: config.gateway.discord_allowed_users.clone(),
        dm_policy: if config.gateway.dm_policy.is_empty() {
            "pairing".to_string()
        } else {
            config.gateway.dm_policy.clone()
        },
        data_dir: config.general.data_dir.clone(),
    };

    let intents = GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    info!("Starting Discord client…");
    let mut client = Client::builder(&token, intents).event_handler(handler).await?;
    client.start().await?;
    Ok(())
}

/// Derive a thread name from the first line of the message (Discord caps at 100).
fn thread_title(content: &str) -> String {
    let first = content.lines().next().unwrap_or(content).trim();
    let mut t: String = first.chars().take(THREAD_NAME_MAX).collect();
    if t.trim().is_empty() {
        t = "conversation".to_string();
    }
    t
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
    fn thread_title_truncates_and_falls_back() {
        assert_eq!(thread_title("hello world"), "hello world");
        assert!(thread_title(&"x".repeat(200)).chars().count() <= THREAD_NAME_MAX);
        assert_eq!(thread_title("   "), "conversation");
    }

    #[test]
    fn chunking_splits_long_messages() {
        let big = "x".repeat(5000);
        let chunks = chunk_message(&big);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.len() <= DISCORD_MAX_LEN));
    }
}
