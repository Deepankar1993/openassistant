# openAssistant — Architecture & Feature Plan

## Vision
A unified personal AI assistant in Rust, combining the best of:
- **OpenHumans**: Data exploration, sharing, project-based collaboration, member management
- **Hermes Agent**: Self-improving agent with memory, skills, cron, multi-platform messaging, subagent delegation
- **OpenClaw**: Multi-channel inbox, voice wake/talk mode, live canvas, companion apps, DM pairing/security

## Core Architecture

```
open-assistant/
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── core/                     # Core agent engine
│   │   ├── agent.rs              # Main agent loop
│   │   ├── session.rs            # Session management
│   │   ├── context.rs            # Conversation context
│   │   └── mod.rs
│   ├── gateway/                  # Messaging gateway
│   │   ├── mod.rs
│   │   ├── discord.rs            # Discord integration
│   │   ├── telegram.rs           # Telegram integration
│   │   ├── slack.rs              # Slack integration
│   │   ├── signal.rs             # Signal integration
│   │   └── webchat.rs            # Web chat interface
│   ├── memory/                   # Persistent memory system
│   │   ├── store.rs              # SQLite-backed storage
│   │   ├── search.rs             # FTS5 search
│   │   └── mod.rs
│   ├── skills/                   # Skills system
│   │   ├── engine.rs             # Skill loading/execution
│   │   ├── builtin/              # Built-in skills
│   │   └── mod.rs
│   ├── cron/                     # Scheduled automations
│   │   ├── scheduler.rs          # Cron job scheduler
│   │   └── mod.rs
│   ├── tools/                    # Tool system
│   │   ├── browser.rs            # Browser automation
│   │   ├── shell.rs              # Shell command execution
│   │   ├── file.rs               # File operations
│   │   ├── vision.rs             # Gemini CLI vision integration
│   │   └── mod.rs
│   ├── platforms/                # Multi-platform support
│   │   ├── mod.rs
│   │   └── data_sources.rs       # OpenHumans-style data sources
│   ├── canvas/                   # Live canvas (OpenClaw-style)
│   │   ├── renderer.rs
│   │   └── mod.rs
│   ├── security/                 # Security model
│   │   ├── pairing.rs            # DM pairing
│   │   ├── allowlist.rs          # Access control
│   │   └── mod.rs
│   ├── onboarding/               # First-run wizard
│   │   ├── wizard.rs
│   │   └── mod.rs
│   └── ui/                       # TUI interface
│       ├── app.rs
│       ├── chat.rs
│       └── mod.rs
├── skills/                       # Skill markdown files
│   ├── coding.md
│   ├── research.md
│   └── writing.md
├── config/                       # Default configs
│   └── default.yaml
└── tests/                        # Integration tests
    └── mod.rs
```

## Key Features

### From OpenHumans
- **Data Management**: Upload, explore, and share personal data
- **Projects**: Create and join research/collaboration projects
- **Member System**: User profiles, membership management
- **Community**: Activity feeds, member lists, data sharing
- **File Upload**: Typed file uploads with metadata

### From Hermes Agent
- **Self-Improving Loop**: Memory persistence, skill creation, session search
- **Multi-Platform Gateway**: Telegram, Discord, Slack, WhatsApp, Signal, Email
- **Skills System**: Auto-create skills from experience, skill improvement
- **Cron Scheduler**: Scheduled tasks with cross-platform delivery
- **Subagent Delegation**: Spawn isolated parallel workstreams
- **Model Agnostic**: Support multiple LLM providers (OpenRouter, OpenAI, etc.)
- **Tool System**: 60+ tools (browser, shell, file, vision via Gemini CLI)
- **Honcho Integration**: Dialectic user modeling
- **Session Search**: FTS5-backed conversation history

### From OpenClaw
- **Multi-Channel Inbox**: 20+ messaging platforms
- **Voice Wake**: Wake word detection
- **Live Canvas**: Agent-driven visual workspace
- **Companion Apps**: System tray / menu bar app
- **Security Model**: DM pairing, allowlist, secure defaults
- **Multi-Agent Routing**: Route channels to isolated agents
- **Skills + Onboarding**: Guided setup wizard

## Implementation Phases

### Phase 1: Core Foundation
- Workspace structure with all crates
- Agent engine with tool system
- Session and context management
- Configuration system (YAML)
- CLI interface

### Phase 2: Memory + Skills
- SQLite-backed persistent memory
- FTS5 search across sessions
- Skill loading and execution engine
- Built-in skills

### Phase 3: Gateway + Messaging
- Discord integration
- Telegram integration
- Slack integration
- Web chat interface

### Phase 4: Advanced Features
- Cron scheduler
- Subagent delegation
- Vision via Gemini CLI
- Live canvas
- Security model (pairing, allowlist)

### Phase 5: Onboarding + Polish
- Interactive onboarding wizard
- TUI interface
- System tray companion
- Documentation
