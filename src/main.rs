// src/main.rs
//! openAssistant CLI binary. The agent core lives in the `open_assistant`
//! library crate (`src/lib.rs`); this binary links it and exposes the
//! `clap`-based command surface.
use open_assistant::{config, gateway, onboarding, skills, tools, ui};

use clap::Parser;
use tracing::info;

#[derive(Parser, Debug)]
#[command(name = "openassistant", version, about = "openAssistant — Your personal AI assistant")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand, Debug)]
enum Commands {
    /// Interactive terminal UI (TUI)
    Tui,
    /// Web-based UI
    Web {
        #[arg(long, default_value = "3000")] port: u16,
    },
    Chat,
    /// Send a one-shot prompt through the local Claude Code CLI bridge
    Claude {
        /// The prompt to send to `claude -p`
        prompt: String,
        /// Resume a prior Claude session id for continuity
        #[arg(long)] resume: Option<String>,
    },
    /// Run the messaging gateway (WebChat + Discord/Telegram/Slack if configured)
    Gateway {
        /// Print the gateway readiness report and exit without starting servers.
        #[arg(long)]
        check: bool,
    },
    /// Generate the daily brief now and print it (scheduled delivery is the
    /// gateway's proactive loop — config [brief]).
    Brief,
    /// Inspect/test MCP servers from <data_dir>/.mcp.json.
    Mcp {
        /// list | call (default: list)
        #[arg(long)] action: Option<String>,
        /// For call: server name
        #[arg(long)] server: Option<String>,
        /// For call: tool name
        #[arg(long)] tool: Option<String>,
        /// For call: JSON arguments (default {})
        #[arg(long)] args: Option<String>,
    },
    /// Manage scheduled jobs (cron.json). The gateway's proactive loop runs due
    /// jobs and delivers results; `--action run` runs due jobs now.
    Cron {
        /// list | add | remove | run (default: list)
        #[arg(long)] action: Option<String>,
        #[arg(long)] name: Option<String>,
        /// e.g. "every 60m", "every 2h", "every 1d"
        #[arg(long)] schedule: Option<String>,
        /// The prompt to run on schedule
        #[arg(long)] task: Option<String>,
        /// For remove: the job id (prefix ok)
        #[arg(long)] id: Option<String>,
    },
    /// Manage standing orders (persistent triggers in standing_orders.json).
    #[command(alias = "orders")]
    StandingOrders {
        /// list | add | remove (default: list)
        #[arg(long)] action: Option<String>,
        /// For add: natural language, e.g. "when i mention deploy, then run ./deploy.sh"
        #[arg(long)] text: Option<String>,
        /// For remove: the order id (see list)
        #[arg(long)] id: Option<String>,
    },
    Onboard,
    Config {
        #[arg(long)] key: Option<String>,
        #[arg(long)] value: Option<String>,
    },
    Status,
    Doctor,
    /// Update openAssistant from its source checkout (git pull + cargo build).
    /// Source checkouts only; set OPENASSISTANT_SRC if the checkout is not
    /// discoverable from the running executable.
    Update {
        /// Only check for pending updates; do not apply them.
        #[arg(long)]
        check: bool,
        /// Skip the confirmation prompt and apply directly.
        #[arg(long)]
        yes: bool,
    },
    /// Read/update MEMORY.md directly
    Memory {
        #[arg(long)] action: Option<String>,
        #[arg(long)] content: Option<String>,
    },
    /// List/read/create skills
    Skills {
        #[arg(long)] action: Option<String>,
        #[arg(long)] name: Option<String>,
        #[arg(long)] content: Option<String>,
    },
    /// Manage sub-agents
    Agents {
        #[arg(long)] action: Option<String>,
        #[arg(long)] name: Option<String>,
    },
    /// Manage plugins
    Plugins {
        #[arg(long)] action: Option<String>,
        #[arg(long)] name: Option<String>,
    },
    /// Run a workflow (real LLM step execution; runs persisted to workflows.db)
    Workflow {
        name: String,
        /// Input text passed as context to the workflow's root steps
        #[arg(long)] input: Option<String>,
    },
    /// Manage goals and subgoals (persisted in goals.json)
    Goals {
        /// list | create | subgoal | task
        #[arg(long)] action: Option<String>,
        /// Goal id-prefix or title (for subgoal/task)
        #[arg(long)] goal: Option<String>,
        /// Subgoal id-prefix or title (for task)
        #[arg(long)] subgoal: Option<String>,
        /// Title of the goal/subgoal/task being created
        #[arg(long)] title: Option<String>,
        /// Optional description
        #[arg(long)] description: Option<String>,
    },
    /// Manage checkpoints (persisted in checkpoints.db)
    Checkpoint {
        /// list | create | restore
        #[arg(long)] action: Option<String>,
        /// Checkpoint id (for restore)
        #[arg(long)] id: Option<String>,
        /// Session id (defaults to a stable hash of the workspace path)
        #[arg(long)] session: Option<String>,
        /// Workspace directory (defaults to the current directory)
        #[arg(long)] workspace: Option<String>,
        /// Comma-separated file paths relative to the workspace (for create)
        #[arg(long)] files: Option<String>,
        /// Checkpoint description (for create)
        #[arg(long)] description: Option<String>,
        /// Overwrite files modified since the checkpoint (for restore)
        #[arg(long)] force: bool,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,open_assistant=debug")
        .init();

    let cli = Cli::parse();
    info!("openAssistant v{} starting", env!("CARGO_PKG_VERSION"));

    match cli.command {
        Commands::Chat => {
            ui::tui::run_tui().await?;
        }
        Commands::Claude { prompt, resume } => {
            let config = config::load().await?;
            // Local operator (this CLI) is trusted with full autonomy
            // (e.g. claude.skip_permissions). Remote callers never are.
            let bridge = open_assistant::core::claude_bridge::ClaudeBridge::from_config(
                &config.claude,
                &config.general.data_dir,
            )
            .operator();
            if !bridge.available().await {
                println!("❌ `claude` binary not found. Set it with `config --key claude.bin --value <path>`.");
            } else {
                println!("⏳ Running claude (cwd: {})…", bridge.workspace());
                match bridge.run(&prompt, resume.as_deref()).await {
                    Ok(r) => {
                        println!("\n{}", r.text);
                        if let Some(sid) = r.session_id {
                            println!("\n[session: {} — resume with `--resume {}`]", sid, sid);
                        }
                        if let Some(cost) = r.cost_usd {
                            println!("[cost: ${:.4}]", cost);
                        }
                    }
                    Err(e) => println!("❌ {}", e),
                }
            }
        }
        Commands::Gateway { check } => {
            let config = config::load().await?;
            // Surface the readiness report on the terminal (always), then either
            // exit (--check) or start the servers.
            print!("{}", gateway::format_readiness(&gateway::readiness(&config)));
            if check {
                return Ok(());
            }
            println!();
            gateway::start_gateway().await?;
        }
        Commands::Onboard => {
            onboarding::wizard::run_wizard().await?;
        }
        Commands::Config { key, value } => {
            if let (Some(k), Some(v)) = (key, value) {
                config::set(&k, &v).await?;
                println!("Set {} = {}", k, v);
            } else {
                config::show_all().await?;
            }
        }
        Commands::Status => {
            println!("openAssistant v{}", env!("CARGO_PKG_VERSION"));
            let config = config::load().await?;
            println!("Agent name: {}", config.general.name);
            println!("Provider: {}", config.model.provider);
            println!("Model: {}", config.model.model);
            println!("Data dir: {}", config.general.data_dir);

            let mem = open_assistant::memory::store::MemoryStore::open_in(&config.general.data_dir)?;
            match mem.count() {
                Ok(c) => println!("Memory entries: {}", c),
                Err(e) => println!("Memory: {}", e),
            }

            // Check workspace memory
            let ws = open_assistant::core::memory::MemoryWorkspace::from_data_dir(&config.general.data_dir);
            let lt = ws.read_long_term();
            if !lt.is_empty() {
                println!("MEMORY.md: {} chars", lt.len());
            }
        }
        Commands::Doctor => {
            run_diagnostics().await?;
        }
        Commands::Update { check, yes } => {
            run_update(check, yes).await?;
        }
        Commands::Memory { action, content } => {
            let config = config::load().await?;
            let ws = open_assistant::core::memory::MemoryWorkspace::from_data_dir(&config.general.data_dir);
            ws.init()?;

            match action.as_deref() {
                Some("read") => {
                    let lt = ws.read_long_term();
                    let today = ws.read_today();
                    println!("--- MEMORY.md ---\n{}", lt);
                    println!("--- Today ---\n{}", today);
                }
                Some("write") => {
                    if let Some(c) = content {
                        ws.write_long_term(&c)?;
                        println!("MEMORY.md updated.");
                    } else {
                        println!("Provide --content for write action.");
                    }
                }
                Some("append") => {
                    if let Some(c) = content {
                        ws.append_long_term(&c)?;
                        println!("Appended to MEMORY.md.");
                    } else {
                        println!("Provide --content for append action.");
                    }
                }
                Some("search") => {
                    if let Some(query) = content {
                        let results = ws.search_files(&query);
                        if results.is_empty() {
                            println!("No results for '{}'", query);
                        } else {
                            for (file, line) in results {
                                println!("[{}] {}", file, line);
                            }
                        }
                    } else {
                        println!("Provide --content for search query.");
                    }
                }
                _ => {
                    println!("Memory commands:");
                    println!("  openassistant memory --action read");
                    println!("  openassistant memory --action write --content \"text\"");
                    println!("  openassistant memory --action append --content \"text\"");
                    println!("  openassistant memory --action search --content \"query\"");
                }
            }
        }
        Commands::Skills { action, name, content } => {
            let config = config::load().await?;
            let dir = &config.general.data_dir;

            match action.as_deref() {
                Some("list") => {
                    let mut skills = Vec::new();
                    if let Ok(mut entries) = tokio::fs::read_dir(format!("{}/skills", dir)).await {
                        while let Ok(Some(e)) = entries.next_entry().await {
                            if let Some(n) = e.file_name().to_str() {
                                skills.push(n.to_string());
                            }
                        }
                    }
                    println!("Skills: {}", if skills.is_empty() { "(none)".to_string() } else { skills.join(", ") });
                }
                Some("read") => {
                    if let Some(n) = name {
                        let path = format!("{}/skills/{}", dir, n);
                        let c = tokio::fs::read_to_string(&path).await.unwrap_or_else(|_| "Not found".to_string());
                        println!("{}", c);
                    }
                }
                Some("create") => {
                    if let (Some(n), Some(c)) = (name, content) {
                        tokio::fs::create_dir_all(format!("{}/skills", dir)).await?;
                        tokio::fs::write(format!("{}/skills/{}", dir, n), c).await?;
                        println!("Created skill: {}", n);
                    }
                }
                _ => {
                    println!("Skill commands:");
                    println!("  openassistant skills --action list");
                    println!("  openassistant skills --action read --name SKILL.md");
                    println!("  openassistant skills --action create --name SKILL.md --content \"text\"");
                }
            }
        }
        Commands::Tui => {
            ui::tui::run_tui().await?;
        }
        Commands::Brief => {
            let config = config::load().await?;
            let store = open_assistant::core::watchers::WatcherStore::open(&config.general.data_dir);
            let recent = open_assistant::core::brief::recent_watcher_summary(&store);
            match open_assistant::core::brief::generate_brief(&config, &recent).await {
                Ok(text) => println!("☀️ Daily brief\n\n{}", text),
                Err(e) => {
                    eprintln!("Could not generate the brief: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Commands::Mcp { action, server, tool, args } => {
            // Local operator CLI: full autonomy (like `claude`). `mcp call` runs
            // a tool directly, outside the agent's permission gate — the remote
            // gateway path keeps the gate (MCP tools need an allow rule there).
            use open_assistant::core::mcp::McpRegistry;
            let config = config::load().await?;
            let mut registry = McpRegistry::open_default(&config.general.data_dir)?;
            if registry.is_empty() {
                println!("No MCP servers configured. Add them to {}/.mcp.json (key \"mcpServers\").", config.general.data_dir);
            } else {
                let n = registry.initialize_all().await?;
                println!("Initialized {} MCP server(s).\n", n);
                match action.as_deref().unwrap_or("list") {
                    "call" => match (server, tool) {
                        (Some(s), Some(t)) => {
                            let parsed = match args.as_deref() {
                                Some(a) => match serde_json::from_str::<serde_json::Value>(a) {
                                    Ok(v) => Some(v),
                                    Err(e) => {
                                        println!("Invalid --args JSON: {}", e);
                                        None
                                    }
                                },
                                None => Some(serde_json::json!({})),
                            };
                            if let Some(parsed) = parsed {
                                let prefixed = format!("mcp__{}__{}", s, t);
                                match registry.call_prefixed(&prefixed, parsed).await {
                                    Ok(out) => println!("{}", out),
                                    Err(e) => println!("⚠️ {}", e),
                                }
                            }
                        }
                        _ => println!("call requires --server and --tool (and optional --args '<json>')."),
                    },
                    _ => {
                        for srv in registry.list_servers() {
                            println!("● {} ({} tools)", srv.server_name, srv.tools.len());
                            for t in &srv.tools {
                                println!("    mcp__{}__{} — {}", srv.server_name, t.name, t.description);
                            }
                        }
                    }
                }
            }
        }
        Commands::Cron { action, name, schedule, task, id } => {
            use open_assistant::cron::scheduler::CronScheduler;
            let config = config::load().await?;
            let dd = &config.general.data_dir;
            let mut scheduler = CronScheduler::load(dd);
            match action.as_deref().unwrap_or("list") {
                "add" => match (name, schedule, task) {
                    (Some(n), Some(s), Some(t)) => {
                        if !open_assistant::cron::scheduler::is_valid_schedule(&s) {
                            println!("Invalid schedule '{}'. Use \"every <N>m|h|d\" (e.g. \"every 60m\").", s);
                        } else {
                            let jid = scheduler.add_job(&n, &s, &t, None);
                            scheduler.save(dd)?;
                            println!("✅ Added cron job '{}' [{}] ({}).", n, &jid[..8.min(jid.len())], s);
                        }
                    }
                    _ => println!("add requires --name, --schedule (e.g. \"every 60m\"), and --task."),
                },
                "remove" => match id {
                    Some(i) if scheduler.remove_job(&i) => {
                        scheduler.save(dd)?;
                        println!("Removed cron job {}.", i);
                    }
                    _ => println!("No cron job with that id (see `cron --action list`)."),
                },
                "run" => {
                    let due = scheduler.take_due(chrono::Utc::now());
                    scheduler.save(dd)?;
                    if due.is_empty() {
                        println!("No cron jobs are due right now.");
                    }
                    for job in due {
                        println!("⏰ Running '{}'…", job.name);
                        let agent = open_assistant::core::agent::Agent::new(config.model.model.clone())
                            .with_workspace(dd.clone())
                            .with_tools_enabled(config.tools.enabled);
                        let mut jctx = open_assistant::core::persona::FullContext::new();
                        let mut jsession = open_assistant::core::session::Session::new("cron", "cli");
                        match agent.process(&job.task, &mut jctx, &mut jsession).await {
                            Ok(r) => println!("{}\n", r),
                            Err(e) => println!("⚠️ {}\n", e),
                        }
                    }
                }
                _ => {
                    let jobs = scheduler.list_jobs();
                    if jobs.is_empty() {
                        println!("No cron jobs. Add one with `cron --action add --name … --schedule \"every 60m\" --task \"…\"`.");
                    }
                    for j in jobs {
                        println!("[{}] {} — {} (enabled={}, runs={}) :: {}", &j.id[..8.min(j.id.len())], j.name, j.schedule, j.enabled, j.run_count, j.task);
                    }
                }
            }
        }
        Commands::StandingOrders { action, text, id } => {
            use open_assistant::core::standing_orders::StandingOrdersEngine;
            let config = config::load().await?;
            let dd = &config.general.data_dir;
            let mut engine = StandingOrdersEngine::load(dd);
            match action.as_deref().unwrap_or("list") {
                "add" => {
                    let t = text.unwrap_or_default();
                    match StandingOrdersEngine::parse_from_text(&t) {
                        Some(order) => {
                            let oid = order.id.clone();
                            engine.add(order);
                            engine.save(dd)?;
                            println!("✅ Added standing order [{}].", &oid[..8.min(oid.len())]);
                        }
                        None => println!("Could not parse. Try: \"when i mention X or Y, then remember it\""),
                    }
                }
                "remove" => match id {
                    Some(i) if engine.remove(&i) => {
                        engine.save(dd)?;
                        println!("Removed standing order {}.", i);
                    }
                    _ => println!("No standing order with that id (see `standing-orders --action list`)."),
                },
                _ => {
                    let orders = engine.list();
                    if orders.is_empty() {
                        println!("No standing orders.");
                    }
                    for o in orders {
                        println!("[{}] {} (enabled={}) — {}", &o.id[..8.min(o.id.len())], o.name, o.enabled, o.description);
                    }
                }
            }
        }
        Commands::Web { port } => {
            // The `web` command IS the gateway WebChat (real agent loop) — the
            // old simulated-response demo UI was removed.
            let config = config::load().await?;
            gateway::webchat::start_on(config, Some(port), None).await?;
        }
        Commands::Agents { action, name } => {
            let config = config::load().await?;
            let workspace = &config.general.data_dir;
            match action.as_deref() {
                Some("list") => {
                    let mut orchestrator = open_assistant::core::subagent::SubAgentOrchestrator::new();
                    let _ = orchestrator.load_definitions(&format!("{}/.claude/agents", workspace));
                    println!("📋 Sub-agent definitions:");
                    for def in orchestrator.list_definitions() {
                        println!("  🤖 {} — {}", def.name, def.description);
                    }
                }
                Some("load") => {
                    if let Some(n) = name {
                        let mut orchestrator = open_assistant::core::subagent::SubAgentOrchestrator::new();
                        let _ = orchestrator.load_definitions(&format!("{}/.claude/agents", workspace));
                        if let Some(def) = orchestrator.get_definition(&n) {
                            println!("Loaded agent: {} — {}", def.name, def.description);
                        } else {
                            println!("Agent '{}' not found", n);
                        }
                    }
                }
                _ => {
                    println!("Agent commands:");
                    println!("  openassistant agents --action list");
                    println!("  openassistant agents --action load --name <agent-name>");
                }
            }
        }
        Commands::Plugins { action, name } => {
            let config = config::load().await?;
            let workspace = &config.general.data_dir;
            match action.as_deref() {
                Some("list") => {
                    let mut marketplace = open_assistant::core::plugins::PluginMarketplace::new(&format!("{}/.claude/plugins", workspace));
                    let _ = marketplace.load_installed();
                    println!("{}", marketplace.format_status());
                }
                Some("enable") => {
                    if let Some(n) = name {
                        let mut marketplace = open_assistant::core::plugins::PluginMarketplace::new(&format!("{}/.claude/plugins", workspace));
                        let _ = marketplace.load_installed();
                        marketplace.set_enabled(&n, true);
                        println!("Plugin '{}' enabled", n);
                    }
                }
                _ => {
                    println!("Plugin commands:");
                    println!("  openassistant plugins --action list");
                    println!("  openassistant plugins --action enable --name <plugin-name>");
                }
            }
        }
        Commands::Goals { action, goal, subgoal, title, description } => {
            use open_assistant::core::goal_store::GoalStore;
            let mut store = GoalStore::open_default()?;
            match action.as_deref() {
                Some("create") => {
                    let t = title.unwrap_or_else(|| "Untitled Goal".to_string());
                    let id = store.create_goal(&t, description.as_deref().unwrap_or(""))?;
                    println!("🎯 Created goal '{}' [{}]", t, &id[..id.len().min(8)]);
                }
                Some("subgoal") => match (goal, title) {
                    (Some(g), Some(t)) => match store.resolve_goal_id(&g) {
                        Some(gid) => {
                            store.add_subgoal(&gid, &t, description.as_deref().unwrap_or(""))?;
                            println!("➕ Added subgoal '{}' to goal [{}]", t, &gid[..gid.len().min(8)]);
                        }
                        None => println!("No goal matching '{}'.", g),
                    },
                    _ => println!("Usage: goals --action subgoal --goal <id|title> --title \"...\""),
                },
                Some("task") => match (goal, subgoal, title) {
                    (Some(g), Some(sg), Some(t)) => match store.resolve_goal_id(&g) {
                        Some(gid) => {
                            let sgid = store.board.get_goal(&gid).and_then(|gl| {
                                gl.subgoals
                                    .iter()
                                    .find(|s| s.id == sg || s.id.starts_with(&sg) || s.title.eq_ignore_ascii_case(&sg))
                                    .map(|s| s.id.clone())
                            });
                            match sgid {
                                Some(sid) => {
                                    store.add_task(&gid, &sid, &t)?;
                                    println!("✅ Added task '{}' under subgoal [{}]", t, &sid[..sid.len().min(8)]);
                                }
                                None => println!("No subgoal matching '{}'.", sg),
                            }
                        }
                        None => println!("No goal matching '{}'.", g),
                    },
                    _ => println!("Usage: goals --action task --goal <id|title> --subgoal <id|title> --title \"...\""),
                },
                Some("list") | None => println!("{}", store.format()),
                _ => {
                    println!("Goal commands:");
                    println!("  openassistant goals --action list");
                    println!("  openassistant goals --action create --title \"...\" [--description \"...\"]");
                    println!("  openassistant goals --action subgoal --goal <id|title> --title \"...\" [--description \"...\"]");
                    println!("  openassistant goals --action task --goal <id|title> --subgoal <id|title> --title \"...\"");
                }
            }
        }
        Commands::Workflow { name, input } => {
            use open_assistant::core::workflows::{built_in_workflows, WorkflowEngine};
            let config = std::sync::Arc::new(config::load().await?);
            let db_path = format!("{}/workflows.db", config.general.data_dir);
            let mut engine = WorkflowEngine::new_with_config(config, &db_path)?;
            for wf in built_in_workflows() {
                engine.register_workflow(wf);
            }
            match engine.execute(&name, input.as_deref()).await {
                Ok(result) => println!("✅ {}", result),
                Err(e) => println!("❌ Workflow error: {}", e),
            }
        }
        Commands::Checkpoint { action, id, session, workspace, files, description, force } => {
            use open_assistant::core::checkpoint::CheckpointStore;
            use sha2::Digest;
            let config = config::load().await?;
            let workspace_dir = workspace.unwrap_or_else(|| {
                std::env::current_dir()
                    .ok()
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| config.general.data_dir.clone())
            });
            // --session defaults to a stable hash of the absolute workspace path.
            let session_id = session.unwrap_or_else(|| {
                let abs = std::fs::canonicalize(&workspace_dir)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| workspace_dir.clone());
                let h = format!("{:x}", sha2::Sha256::digest(abs.as_bytes()));
                format!("ws_{}", &h[..12])
            });

            let mut store = CheckpointStore::open_default()?;
            match action.as_deref() {
                Some("create") => {
                    let file_list: Vec<String> = files
                        .as_deref()
                        .map(|f| f.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
                        .unwrap_or_default();
                    if file_list.is_empty() {
                        println!("Provide --files a.rs,b.rs to checkpoint.");
                    } else {
                        let desc = description.unwrap_or_else(|| "manual checkpoint".to_string());
                        let cp_id = store.create_checkpoint(&session_id, &desc, &workspace_dir, &file_list)?;
                        println!("📸 Created checkpoint {} (session {}) with {} file(s).", cp_id, session_id, file_list.len());
                    }
                }
                Some("restore") => match id {
                    Some(cp_id) => {
                        let report = store.restore_checkpoint(&cp_id, &workspace_dir, force)?;
                        let skip_note = if report.skipped.is_empty() {
                            String::new()
                        } else {
                            format!(", skipped {} (modified — re-run with --force to overwrite)", report.skipped.len())
                        };
                        println!("✅ Restored {} file(s){}.", report.restored.len(), skip_note);
                        for f in &report.skipped {
                            println!("  ⚠️  skipped: {}", f);
                        }
                    }
                    None => println!("Usage: openassistant checkpoint --action restore --id <checkpoint-id> [--workspace <dir>] [--force]"),
                },
                Some("list") => {
                    println!("{}", store.format_checkpoints(&session_id));
                }
                _ => {
                    println!("Checkpoint commands:");
                    println!("  openassistant checkpoint --action list [--session <id> | --workspace <dir>]");
                    println!("  openassistant checkpoint --action create --files a.rs,b.rs [--workspace <dir>] [--description \"...\"]");
                    println!("  openassistant checkpoint --action restore --id <checkpoint-id> [--workspace <dir>] [--force]");
                }
            }
        }
    }

    Ok(())
}

/// Locate the source checkout to update. Order: `OPENASSISTANT_SRC` env var
/// (primary — an installed binary in `~/.cargo/bin` cannot be walked back to
/// the source), then walk up from the running executable looking for a
/// `Cargo.toml` whose package is `open-assistant`, then error.
fn detect_source_dir() -> anyhow::Result<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("OPENASSISTANT_SRC") {
        let p = std::path::PathBuf::from(dir);
        if p.join("Cargo.toml").exists() {
            return Ok(p);
        }
        anyhow::bail!("OPENASSISTANT_SRC is set to '{}' but no Cargo.toml is there.", p.display());
    }

    if let Ok(exe) = std::env::current_exe() {
        let mut cur = exe.parent().map(|p| p.to_path_buf());
        while let Some(dir) = cur {
            let manifest = dir.join("Cargo.toml");
            if manifest.exists() {
                if let Ok(text) = std::fs::read_to_string(&manifest) {
                    if text.contains("name = \"open-assistant\"") {
                        return Ok(dir);
                    }
                }
            }
            cur = dir.parent().map(|p| p.to_path_buf());
        }
    }

    anyhow::bail!(
        "Could not locate the openAssistant source checkout. `update` works only \
         for source installs. Set OPENASSISTANT_SRC to your checkout directory, e.g.\n  \
         OPENASSISTANT_SRC=/path/to/openAssistant openassistant update"
    )
}

/// Real `update` flow over the existing `SelfUpdater` (git pull + cargo build).
async fn run_update(check_only: bool, assume_yes: bool) -> anyhow::Result<()> {
    use open_assistant::core::self_update::SelfUpdater;

    let src = detect_source_dir()?;
    println!("openAssistant v{} — source: {}", env!("CARGO_PKG_VERSION"), src.display());

    let updater = SelfUpdater::new(src.to_string_lossy().to_string());

    let pending = updater.check_pending().await?;
    if pending.is_empty() {
        println!("✅ Already up to date.");
        return Ok(());
    }

    println!("\n{} pending commit(s) upstream:", pending.len());
    for line in &pending {
        println!("  • {}", line);
    }

    if check_only {
        println!("\nRun `openassistant update` to apply.");
        return Ok(());
    }

    if !assume_yes {
        print!("\nApply update (git pull --rebase + cargo build --release)? [y/N] ");
        use std::io::Write;
        std::io::stdout().flush().ok();
        let mut input = String::new();
        std::io::stdin().read_line(&mut input)?;
        if !matches!(input.trim().to_lowercase().as_str(), "y" | "yes") {
            println!("Aborted.");
            return Ok(());
        }
    }

    if updater.is_dirty().await? {
        anyhow::bail!("Working tree is dirty — commit or stash your changes before updating.");
    }

    println!("Building… (this can take a few minutes)");
    let built = updater.update_from_source().await?;
    if built {
        println!("✅ Update complete. A fresh binary was written to target/release/.");
        println!("   • Restart openAssistant to run the new version (the current process keeps v{}).", env!("CARGO_PKG_VERSION"));
        println!("   • If you installed via `cargo install`, run `cargo install --path .` from {} to update your PATH.", src.display());
    } else {
        println!("✅ Already up to date.");
    }
    Ok(())
}

async fn run_diagnostics() -> anyhow::Result<()> {
    println!("🔍 openAssistant Diagnostics");
    println!("─────────────────────────────");

    match config::check().await {
        Ok(_) => println!("✅ Config: OK"),
        Err(e) => println!("❌ Config: {}", e),
    }

    match open_assistant::memory::store::MemoryStore::open_default().await {
        Ok(_) => println!("✅ Memory DB: OK"),
        Err(e) => println!("❌ Memory DB: {}", e),
    }

    let config = config::load().await?;
    let ws = open_assistant::core::memory::MemoryWorkspace::from_data_dir(&config.general.data_dir);
    match ws.init() {
        Ok(_) => println!("✅ Memory workspace: OK"),
        Err(e) => println!("❌ Memory workspace: {}", e),
    }

    match skills::check().await {
        Ok(c) => println!("✅ Skills: {} loaded", c),
        Err(e) => println!("❌ Skills: {}", e),
    }

    match gateway::check().await {
        Ok(_) => println!("✅ Gateway: configured"),
        Err(e) => println!("⚠️  Gateway: {}", e),
    }

    match tools::vision::check().await {
        Ok(_) => println!("✅ Vision (Gemini CLI): available"),
        Err(e) => println!("⚠️  Vision (Gemini CLI): {}", e),
    }

    println!("─────────────────────────────");
    println!("Diagnostics complete.");
    Ok(())
}
