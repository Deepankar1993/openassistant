// src/ui/chat.rs
use anyhow::Result;
use std::io::{self, Write};

use crate::core::persona::FullContext;
use crate::core::agent::Agent;
use crate::core::session::Session;

pub async fn run_chat() -> Result<()> {
    let config = crate::config::load().await?;
    let data_dir = config.general.data_dir.clone();

    println!("🦞 openAssistant");
    println!("─────────────────");
    println!("Type your message. Commands: /help, /status, /memory, /skills, /exit");
    println!();

    let agent = Agent::new(&config.model.model).with_workspace(&data_dir);

    // Initialize memory workspace
    let mem = crate::core::memory::MemoryWorkspace::from_data_dir(&data_dir);
    mem.init()?;

    let mut ctx = FullContext::new();
    let mut session = Session::new("cli", "local");

    // Load persona from config if set
    ctx.persona.name = config.general.name.clone();
    ctx.user.name = config.general.user_name.clone();

    loop {
        print!("{} ", ctx.persona.emoji);
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        let input = input.trim();

        if input.is_empty() { continue; }

        // Slash commands
        if input.starts_with('/') {
            match input {
                "/exit" | "/quit" => {
                    println!(" Goodbye! 👋");
                    break;
                }
                "/help" => {
                    print_help();
                    continue;
                }
                "/status" => {
                    print_status(&ctx, &session);
                    continue;
                }
                "/memory" => {
                    print_memory(&mem);
                    continue;
                }
                "/skills" => {
                    let updater = crate::core::self_update::SelfUpdater::new(&data_dir);
                    match updater.list_skills().await {
                        Ok(skills) => {
                            println!("📚 Skills:");
                            for s in skills { println!("  - {}", s); }
                        }
                        Err(e) => println!("Error: {}", e),
                    }
                    continue;
                }
                _ => {
                    println!("Unknown command. Try /help");
                    continue;
                }
            }
        }

        // Process through agent
        match agent.process(input, &mut ctx, &mut session).await {
            Ok(response) => {
                println!("{}: {}\n", ctx.persona.emoji, response);
            }
            Err(e) => {
                eprintln!("Error: {}\n", e);
            }
        }
    }

    Ok(())
}

fn print_help() {
    println!("Commands:");
    println!("  /help    — Show this help");
    println!("  /status  — Show session and context status");
    println!("  /memory  — Show MEMORY.md and today's notes");
    println!("  /skills  — List available skills");
    println!("  /exit    — Exit\n");
}

fn print_status(ctx: &FullContext, session: &Session) {
    println!("Session: {} messages on {}", session.messages().len(), session.channel);
    println!("Model sessions: {}", ctx.session_count);
    println!("Topics: {:?}", ctx.topics);
    println!("User: {} ({})\n", ctx.user.name, ctx.user.technical_level);
}

fn print_memory(mem: &crate::core::memory::MemoryWorkspace) {
    let lt = mem.read_long_term();
    if !lt.is_empty() { println!("--- MEMORY.md ---\n{}", lt); }
    let today = mem.read_today();
    if !today.is_empty() { println!("--- Today ---\n{}", today); }
    println!();
}
