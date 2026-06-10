// src/onboarding/wizard.rs
use anyhow::Result;
use tracing::info;

/// Interactive onboarding wizard (OpenClaw-style `openclaw onboard`)
pub async fn run_wizard() -> Result<()> {
    println!("🦞 Welcome to openAssistant!");
    println!("═══════════════════════════════");
    println!();
    println!("Let's get you set up in just a few steps.");
    println!();

    // Step 1: Workspace
    println!("📁 Step 1: Workspace");
    let data_dir = prompt("Data directory", &default_data_dir())?;
    println!("   → Using: {}", data_dir);
    println!();

    // Step 2: LLM Provider
    println!("🤖 Step 2: LLM Provider");
    println!("   Choose your provider:");
    println!("   1. OpenRouter (recommended — 200+ models)");
    println!("   2. OpenAI");
    println!("   3. Custom endpoint");
    let provider_choice = prompt("Choice (1-3)", "1")?;
    let (provider, api_base) = match provider_choice.as_str() {
        "2" => ("openai".to_string(), "https://api.openai.com/v1".to_string()),
        "3" => (
            "custom".to_string(),
            prompt("API base URL", "http://localhost:8080/v1")?,
        ),
        _ => ("openrouter".to_string(), "https://openrouter.ai/api/v1".to_string()),
    };

    let api_key = prompt("API key", "")?;
    let model = if provider == "openrouter" {
        prompt("Model", "openrouter/owl-alpha")?
    } else {
        prompt("Model", "gpt-4o")?
    };
    println!();

    // Step 3: Messaging
    println!("💬 Step 3: Messaging Channels");
    println!("   Configure at least one channel:");
    let discord_token = prompt("Discord bot token (or Enter to skip)", "")?;
    let telegram_token = prompt("Telegram bot token (or Enter to skip)", "")?;
    println!();

    // Step 4: Security
    println!("🔒 Step 4: Security");
    let dm_pairing = prompt("Enable DM pairing? (y/n)", "y")?.to_lowercase() == "y";
    println!();

    // Step 4.5: User identity
    println!("👤 Step 4: Your Identity");
    let user_name = prompt("What should I call you?", "friend")?;
    println!();

    // Step 5: Tools
    println!("🔧 Step 5: Tools");
    println!("   Vision via Gemini CLI: checking...");
    match crate::tools::vision::check().await {
        Ok(_) => println!("   ✅ Gemini CLI detected and working!"),
        Err(e) => println!("   ⚠️  Gemini CLI not available: {}", e),
    }
    println!();

    // Step 6: Skills
    println!("📚 Step 6: Skills");
    println!("   Built-in skills: coding, research, writing");
    let skill_dirs = prompt("Additional skill directories (comma-separated, or Enter to skip)", "")?;
    println!();

    // Save config
    println!("💾 Saving configuration...");
    let config = crate::config::Config {
        general: crate::config::GeneralConfig {
            data_dir: data_dir.clone(),
            log_level: "info".to_string(),
            name: "openAssistant".to_string(),
            user_name: user_name.clone(),
        },
        model: crate::config::ModelConfig {
            provider: provider.clone(),
            model: model.clone(),
            api_key: api_key.clone(),
            api_base: api_base.clone(),
        },
        gateway: crate::config::GatewayConfig {
            discord_token: discord_token.clone(),
            discord_allowed_users: vec![],
            discord_home_channel: String::new(),
            discord_review_hours: 0,
            telegram_token: telegram_token.clone(),
            slack_token: String::new(),
            slack_signing_secret: String::new(),
            webhook_host: String::new(),
            webhook_port: 3000,
            dm_policy: if dm_pairing { "pairing".to_string() } else { "open".to_string() },
        },
        memory: crate::config::MemoryConfig {
            db_path: format!("{}/memory.db", data_dir),
            max_entries: 100_000,
            fts_enabled: true,
        },
        skills: crate::config::SkillsConfig {
            dirs: if skill_dirs.is_empty() {
                vec![format!("{}/skills", data_dir)]
            } else {
                skill_dirs.split(',').map(|s| s.trim().to_string()).collect()
            },
            auto_create: true,
        },
        security: crate::config::SecurityConfig {
            dm_pairing,
            allow_from: vec![],
        },
        vision: crate::config::VisionConfig {
            provider: "gemini-cli".to_string(),
            gemini_path: "gemini".to_string(),
        },
        tools: crate::config::ToolsConfig::default(),
        providers: vec![],
        routing: crate::config::RoutingConfig::default(),
        claude: crate::config::ClaudeBridgeConfig::default(),
        permissions: crate::config::PermissionsConfig::default(),
    };

    crate::config::save(&config).await?;
    println!("   ✅ Configuration saved!");
    println!();

    // Summary
    println!("═══════════════════════════════");
    println!("🎉 Setup complete! You can now:");
    println!("   openassistant chat        — Start interactive chat");
    println!("   openassistant gateway     — Start messaging gateway");
    println!("   openassistant onboard     — Re-run this wizard");
    println!("   openassistant config      — View/edit config");
    println!("   openassistant status      — Check status");
    println!("   openassistant doctor      — Run diagnostics");
    println!();
    println!("   WebChat: http://localhost:3000");
    println!();

    Ok(())
}

fn prompt(question: &str, default: &str) -> Result<String> {
    use std::io::{self, Write};

    if default.is_empty() {
        print!("{}: ", question);
    } else {
        print!("{} [{}]: ", question, default);
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim();

    if input.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(input.to_string())
    }
}

fn default_data_dir() -> String {
    format!(
        "{}/.openassistant",
        std::env::var("HOME")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".to_string())
    )
}
