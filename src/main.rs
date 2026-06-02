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
    Gateway,
    Onboard,
    Config {
        #[arg(long)] key: Option<String>,
        #[arg(long)] value: Option<String>,
    },
    Status,
    Doctor,
    Update,
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
    /// Run a workflow
    Workflow {
        name: String,
    },
    /// Manage checkpoints
    Checkpoint {
        #[arg(long)] action: Option<String>,
        #[arg(long)] id: Option<String>,
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
        Commands::Gateway => {
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

            let mem = open_assistant::memory::store::MemoryStore::open_default().await?;
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
        Commands::Update => {
            println!("Use 'cargo update && cargo build --release' to update from source.");
            println!("Or run 'openassistant onboard' to reconfigure.");
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
        Commands::Web { port } => {
            ui::web::run_web(port).await?;
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
        Commands::Workflow { name } => {
            let mut engine = open_assistant::core::workflows::WorkflowEngine::new();
            for wf in open_assistant::core::workflows::built_in_workflows() {
                engine.register_workflow(wf);
            }
            match engine.execute(&name).await {
                Ok(result) => println!("✅ {}", result),
                Err(e) => println!("❌ Workflow error: {}", e),
            }
        }
        Commands::Checkpoint { action, id } => {
            let config = config::load().await?;
            let workspace = &config.general.data_dir;
            let mut store = open_assistant::core::checkpoint::CheckpointStore::new();
            match action.as_deref() {
                Some("list") => {
                    if let Some(session_id) = id {
                        println!("{}", store.format_checkpoints(&session_id));
                    } else {
                        println!("Usage: openassistant checkpoint --action list --id <session-id>");
                    }
                }
                _ => {
                    println!("Checkpoint commands:");
                    println!("  openassistant checkpoint --action list --id <session-id>");
                }
            }
        }
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
