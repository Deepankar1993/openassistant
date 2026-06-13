// src/gateway/discord.rs
//! Discord gateway (Hermes-style) via `serenity`.
//!
//! - **@mention** the bot (or post in the **home** channel) → react ✅, spawn a
//!   **thread**, reply inside it. Messages inside a bot-owned thread continue
//!   that conversation. **DMs** are answered directly.
//! - **Slash commands**: `/ask`, `/home`, `/unset_home`, `/new`, `/help`
//!   (registered per-guild on connect). Text commands still work too.
//! - **Persistence**: owned threads + each conversation's `Session` are stored
//!   in `discord.db`, so threads survive restarts.
//! - **Self-improvement review**: when `gateway.discord_review_hours > 0`, a
//!   periodic task posts a short reflection to the home channel and appends it
//!   to memory.

use anyhow::Result;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};

use serenity::all::{
    ChannelId, Client, CommandDataOptionValue, CommandOptionType, Context as SerenityContext,
    CreateCommand, CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseFollowup,
    CreateInteractionResponseMessage, CreateThread, EventHandler, GatewayIntents, Http,
    Interaction, Message as DiscordMessage, ReactionType, Ready,
};
use serenity::async_trait;

use super::discord_store::DiscordStore;
use crate::config::Config;
use crate::core::agent::{call_llm_raw, Agent};
use crate::core::claude_bridge::ClaudeBridge;
use crate::core::persona::{FullContext, Persona};
use crate::core::session::Session;

const MAX_SESSION_MESSAGES: usize = 40;
const DISCORD_MAX_LEN: usize = 1900;
const THREAD_NAME_MAX: usize = 90;

struct Handler {
    agent: Arc<Agent>,
    /// When set, conversations are driven by the local Claude Code CLI instead
    /// of the built-in agent loop.
    bridge: Option<Arc<ClaudeBridge>>,
    store: Arc<Mutex<DiscordStore>>,
    /// In-memory cache of bot-owned thread ids (seeded from the store on start).
    threads: Arc<Mutex<HashSet<u64>>>,
    home_channel: Arc<Mutex<Option<u64>>>,
    allowed_users: Vec<String>,
    dm_policy: String,
    data_dir: String,
}

impl Handler {
    fn fresh_ctx(&self) -> FullContext {
        let mut ctx = FullContext::new();
        ctx.persona = Persona::load_or_default(&self.data_dir);
        ctx
    }

    /// Run text through the Claude Code CLI, persisting its session id per
    /// conversation so the thread maps to one continuous Claude session.
    async fn respond_via_claude(&self, bridge: &ClaudeBridge, conv_id: u64, text: &str) -> String {
        let resume = { self.store.lock().await.get_claude_session(conv_id).ok().flatten() };
        match bridge.run(text, resume.as_deref()).await {
            Ok(r) => {
                if let Some(sid) = &r.session_id {
                    let store = self.store.lock().await;
                    let _ = store.set_claude_session(conv_id, sid);
                }
                if r.text.trim().is_empty() {
                    "…(Claude returned no text — try rephrasing)".to_string()
                } else {
                    r.text
                }
            }
            Err(e) => format!("⚠️ Claude bridge error: {}", e),
        }
    }

    /// Run text through the conversation. Uses the Claude bridge when enabled,
    /// otherwise the built-in agent. The store lock is only held for the brief
    /// load/save — never across `.await` of the model/bridge call.
    async fn respond(&self, conv_id: u64, text: &str) -> String {
        if let Some(bridge) = self.bridge.clone() {
            return self.respond_via_claude(&bridge, conv_id, text).await;
        }
        let mut session = {
            let store = self.store.lock().await;
            store.load_session(conv_id).ok().flatten()
        }
        .unwrap_or_else(|| Session::new("discord", conv_id.to_string()));

        let mut ctx = self.fresh_ctx();
        let reply = match self.agent.process(text, &mut ctx, &mut session).await {
            Ok(r) => r,
            Err(e) => format!("⚠️ {}", e),
        };

        let len = session.messages.len();
        if len > MAX_SESSION_MESSAGES {
            session.messages.drain(0..(len - MAX_SESSION_MESSAGES));
        }
        {
            let store = self.store.lock().await;
            let _ = store.save_session(conv_id, &session);
        }
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

    async fn set_home(&self, channel: u64) {
        *self.home_channel.lock().await = Some(channel);
        if let Ok(mut cfg) = crate::config::load().await {
            cfg.gateway.discord_home_channel = channel.to_string();
            let _ = crate::config::save(&cfg).await;
        }
    }

    async fn clear_home(&self) {
        *self.home_channel.lock().await = None;
        if let Ok(mut cfg) = crate::config::load().await {
            cfg.gateway.discord_home_channel = String::new();
            let _ = crate::config::save(&cfg).await;
        }
    }

    async fn reset_conversation(&self, conv_id: u64) {
        let store = self.store.lock().await;
        let _ = store.clear_conversation(conv_id);
    }

    /// Handle a prefix/text command. Returns true if the message was a command.
    async fn try_command(&self, ctx: &SerenityContext, msg: &DiscordMessage, content: &str) -> bool {
        let norm = content.trim().to_lowercase();
        let channel = msg.channel_id;
        match norm.as_str() {
            "set home" | "!home" | "!sethome" => {
                self.set_home(channel.get()).await;
                self.send_chunked(ctx, channel, "🏠 Home channel set. New messages here will start a thread.").await;
                true
            }
            "unset home" | "!unsethome" => {
                self.clear_home().await;
                self.send_chunked(ctx, channel, "🏠 Home channel cleared.").await;
                true
            }
            "!new" | "!reset" => {
                self.reset_conversation(channel.get()).await;
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
    async fn ready(&self, ctx: SerenityContext, ready: Ready) {
        info!("Discord connected as {}", ready.user.name);
        info!(
            "Note: enable the MESSAGE CONTENT intent and grant Create Public Threads + \
             Send Messages in Threads + Add Reactions, or thread mode won't work."
        );

        // Register slash commands per-guild (instant, unlike global commands).
        let commands = slash_commands();
        for g in &ready.guilds {
            if let Err(e) = g.id.set_commands(&ctx.http, commands.clone()).await {
                warn!("Could not register slash commands in guild {}: {}", g.id, e);
            }
        }
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
        if self.try_command(&ctx, &msg, &content).await {
            return;
        }

        let channel_id = msg.channel_id.get();
        let is_dm = msg.guild_id.is_none();

        if is_dm {
            self.react_ack(&ctx, &msg).await;
            let reply = self.respond(channel_id, &content).await;
            self.send_chunked(&ctx, msg.channel_id, &reply).await;
            return;
        }

        if self.threads.lock().await.contains(&channel_id) {
            self.react_ack(&ctx, &msg).await;
            let reply = self.respond(channel_id, &content).await;
            self.send_chunked(&ctx, msg.channel_id, &reply).await;
            return;
        }

        let mentioned = msg.mentions_me(&ctx.http).await.unwrap_or(false);
        let in_home = *self.home_channel.lock().await == Some(channel_id);
        if mentioned || in_home {
            self.react_ack(&ctx, &msg).await;
            let title = thread_title(&content);
            match msg
                .channel_id
                .create_thread_from_message(&ctx.http, msg.id, CreateThread::new(title.clone()))
                .await
            {
                Ok(thread) => {
                    let tid = thread.id.get();
                    self.threads.lock().await.insert(tid);
                    {
                        let store = self.store.lock().await;
                        let _ = store.mark_thread(tid, &title);
                    }
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
    }

    async fn interaction_create(&self, ctx: SerenityContext, interaction: Interaction) {
        let Interaction::Command(cmd) = interaction else { return };

        if !gate(&cmd.user.id.get().to_string(), &self.allowed_users, &self.dm_policy) {
            let _ = cmd
                .create_response(
                    &ctx.http,
                    CreateInteractionResponse::Message(
                        CreateInteractionResponseMessage::new()
                            .content("You're not on this bot's allowlist.")
                            .ephemeral(true),
                    ),
                )
                .await;
            return;
        }

        let channel_id = cmd.channel_id.get();
        match cmd.data.name.as_str() {
            "ask" => {
                let message = cmd
                    .data
                    .options
                    .iter()
                    .find(|o| o.name == "message")
                    .and_then(|o| match &o.value {
                        CommandDataOptionValue::String(s) => Some(s.clone()),
                        _ => None,
                    })
                    .unwrap_or_default();
                // Defer (LLM call may exceed the 3s interaction deadline).
                if cmd
                    .create_response(&ctx.http, CreateInteractionResponse::Defer(CreateInteractionResponseMessage::new()))
                    .await
                    .is_err()
                {
                    return;
                }
                let reply = self.respond(channel_id, &message).await;
                for chunk in chunk_message(&reply) {
                    let _ = cmd
                        .create_followup(&ctx.http, CreateInteractionResponseFollowup::new().content(chunk))
                        .await;
                }
            }
            "home" => {
                self.set_home(channel_id).await;
                respond_ephemeral(&ctx, &cmd, "🏠 Home channel set.").await;
            }
            "unset_home" => {
                self.clear_home().await;
                respond_ephemeral(&ctx, &cmd, "🏠 Home channel cleared.").await;
            }
            "new" => {
                self.reset_conversation(channel_id).await;
                respond_ephemeral(&ctx, &cmd, "🧹 Started a fresh conversation here.").await;
            }
            "help" => respond_ephemeral(&ctx, &cmd, HELP_TEXT).await,
            _ => {}
        }
    }
}

async fn respond_ephemeral(ctx: &SerenityContext, cmd: &serenity::all::CommandInteraction, text: &str) {
    let _ = cmd
        .create_response(
            &ctx.http,
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new().content(text).ephemeral(true),
            ),
        )
        .await;
}

fn slash_commands() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("ask").description("Ask openAssistant").add_option(
            CreateCommandOption::new(CommandOptionType::String, "message", "Your message").required(true),
        ),
        CreateCommand::new("home").description("Set this channel as the bot's home"),
        CreateCommand::new("unset_home").description("Clear the home channel"),
        CreateCommand::new("new").description("Start a fresh conversation here"),
        CreateCommand::new("help").description("Show openAssistant help"),
    ]
}

const HELP_TEXT: &str = "🦉 openAssistant — Discord:\n\
• @mention me (or post in the home channel) → I spawn a thread and we continue there\n\
• Slash: `/ask`, `/home`, `/unset_home`, `/new`, `/help`\n\
• Text: `set home`/`!home`, `unset home`, `!new`, `!help`\n\
• DM me for a 1:1 chat. Threads & history persist across restarts.";

/// Start the Discord gateway. Blocks until the client disconnects.
pub async fn start(config: Config, mcp: Option<std::sync::Arc<crate::core::mcp::McpRegistry>>) -> Result<()> {
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

    let store = DiscordStore::open_default()?;
    let owned = store.owned_threads().unwrap_or_default();
    info!("Loaded {} persisted Discord thread(s).", owned.len());

    let mut agent = Agent::new(config.model.model.clone())
        .with_workspace(config.general.data_dir.clone())
        .with_tools_enabled(config.tools.enabled)
        .with_permission_mode(crate::core::permissions::PermissionMode::from_str(
            &config.permissions.gateway_mode,
        ));
    if let Some(m) = &mcp {
        agent = agent.with_mcp(m.clone());
    }

    let home = config.gateway.discord_home_channel.trim().parse::<u64>().ok();

    // Build the Claude bridge if enabled, injecting persona + a human tone so
    // replies feel like a friendly teammate rather than a task runner.
    let bridge = if config.claude.enabled && config.claude.discord_default {
        let persona = Persona::load_or_default(&config.general.data_dir);
        let human = build_human_prompt(&persona, &config.claude.append_system_prompt);
        // Discord is a REMOTE origin (not the local operator): the bridge caps
        // it to a non-bypass permission mode and never honors skip_permissions,
        // so an allowlisted chat author can't escalate to full autonomy.
        let b = ClaudeBridge::from_config(&config.claude, &config.general.data_dir).with_system_prompt(human);
        if config.gateway.dm_policy == "open" && config.gateway.discord_allowed_users.is_empty() {
            warn!(
                "SECURITY: Claude bridge is ON with dm_policy=open and no allowlist — anyone who \
                 can DM the bot can drive Claude Code on this machine. Set gateway.discord_allowed_users."
            );
        }
        if config.claude.skip_permissions {
            warn!(
                "Note: claude.skip_permissions applies only to the local `openassistant claude` CLI; \
                 Discord-driven calls are capped to a non-bypass permission mode."
            );
        }
        if b.available().await {
            info!("Claude bridge ON — Discord conversations route through `claude` (cwd: {}).", b.workspace());
            Some(Arc::new(b))
        } else {
            warn!("Claude bridge enabled but the `claude` binary was not found; using the built-in agent.");
            None
        }
    } else {
        None
    };

    let handler = Handler {
        agent: Arc::new(agent),
        bridge,
        store: Arc::new(Mutex::new(store)),
        threads: Arc::new(Mutex::new(owned)),
        home_channel: Arc::new(Mutex::new(home)),
        allowed_users: config.gateway.discord_allowed_users.clone(),
        dm_policy: if config.gateway.dm_policy.is_empty() {
            "pairing".to_string()
        } else {
            config.gateway.dm_policy.clone()
        },
        data_dir: config.general.data_dir.clone(),
    };

    // GUILDS is needed so `ready.guilds` is populated for per-guild slash-command
    // registration; the rest receive messages + their content.
    let intents = GatewayIntents::GUILDS
        | GatewayIntents::GUILD_MESSAGES
        | GatewayIntents::DIRECT_MESSAGES
        | GatewayIntents::MESSAGE_CONTENT;

    info!("Starting Discord client…");
    let mut client = Client::builder(&token, intents).event_handler(handler).await?;

    // Periodic self-improvement review (cron-style) posting to the home channel.
    if config.gateway.discord_review_hours > 0 {
        let http = client.http.clone();
        tokio::spawn(review_loop(http, config.clone()));
    }

    client.start().await?;
    Ok(())
}

/// Periodically post a short self-improvement reflection to the home channel and
/// append it to memory. Re-reads config each tick so the home channel/interval
/// can change without a restart.
async fn review_loop(http: Arc<Http>, initial: Config) {
    let hours = initial.gateway.discord_review_hours.max(1);
    let mut tick = tokio::time::interval(std::time::Duration::from_secs(hours * 3600));
    tick.tick().await; // consume the immediate first tick — wait a full period first
    loop {
        tick.tick().await;
        let cfg = match crate::config::load().await {
            Ok(c) => c,
            Err(_) => continue,
        };
        if cfg.gateway.discord_review_hours == 0 {
            continue;
        }
        let Some(home) = cfg.gateway.discord_home_channel.trim().parse::<u64>().ok() else {
            continue;
        };
        let review = generate_review(&cfg).await;
        let msg = format!("🪞 **Self-improvement review:** {}", review);
        for chunk in chunk_message(&msg) {
            let _ = ChannelId::new(home).say(&http, chunk).await;
        }
    }
}

/// Produce a brief reflection from memory, append it to the daily note (so
/// "memory updated" is literally true), and return a one-liner for posting.
async fn generate_review(cfg: &Config) -> String {
    let ws = crate::core::memory::MemoryWorkspace::from_data_dir(&cfg.general.data_dir);
    let lt = ws.read_long_term();
    let today = ws.read_today();
    let trunc = |s: String, n: usize| s.chars().take(n).collect::<String>();

    let prompt = format!(
        "Write a 2-3 sentence self-improvement reflection: what you learned recently and one thing \
         to improve. Be concise and concrete.\n\n# MEMORY\n{}\n\n# TODAY\n{}",
        trunc(lt, 2000),
        trunc(today, 2000)
    );

    let client = reqwest::Client::new();
    let (base, key, model) = crate::config::resolve_provider(cfg, "text");
    let summary = call_llm_raw(
        &client,
        base,
        key,
        model,
        &[serde_json::json!({ "role": "user", "content": prompt })],
    )
    .await
    .unwrap_or_default();

    let summary = summary.trim().to_string();
    let line = if summary.is_empty() { "Memory updated.".to_string() } else { summary };
    let _ = ws.append_daily(&format!("Self-improvement review: {}", line));
    line
}

/// Compose a human, persona-flavored system prompt appended to Claude's, so
/// bridged replies feel like a warm teammate rather than a task runner.
fn build_human_prompt(persona: &Persona, extra: &str) -> String {
    let mut p = format!(
        "You are {} (tone: {}). {} You're chatting with your user on Discord. \
         Reply like a warm, friendly human teammate — natural, concise, and personable. \
         Avoid robotic preambles like 'As an AI'. Mirror the user's tone. When you do real \
         work (code, files, commands), briefly say what you did in plain language.",
        persona.name, persona.tone, persona.personality
    );
    if !extra.trim().is_empty() {
        p.push_str("\n\n");
        p.push_str(extra);
    }
    p
}

fn thread_title(content: &str) -> String {
    let first = content.lines().next().unwrap_or(content).trim();
    let mut t: String = first.chars().take(THREAD_NAME_MAX).collect();
    if t.trim().is_empty() {
        t = "conversation".to_string();
    }
    t
}

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
    fn slash_commands_present() {
        let names: Vec<_> = slash_commands().iter().map(|_| ()).collect();
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn chunking_splits_long_messages() {
        let big = "x".repeat(5000);
        let chunks = chunk_message(&big);
        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|c| c.len() <= DISCORD_MAX_LEN));
    }
}
