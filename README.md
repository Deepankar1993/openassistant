# 🦞 openAssistant

**Your own personal AI assistant. Any OS. Any Platform.**

A unified personal AI assistant in Rust, combining the best of:
- **[OpenHumans](https://www.openhumans.org)** — Data exploration, sharing, projects, community
- **[Hermes Agent](https://hermes-agent.nousresearch.com)** — Self-improving agent with memory, skills, cron, multi-platform messaging
- **[OpenClaw](https://github.com/openclaw/openclaw)** — Multi-channel inbox, voice wake, live canvas, DM pairing, companion apps

## Features

### 🧠 Self-Improving Agent
- **Memory**: SQLite-backed persistent memory with FTS5 search
- **Skills**: Auto-create skills from experience, improve during use
- **Session Search**: Full-text search across all past conversations
- **User Modeling**: Builds a deepening model of who you are

### 💬 Multi-Platform Gateway
- **Discord**, **Telegram**, **Slack**, **Signal**, **WebChat**
- DM pairing mode for security (OpenClaw-style)
- Cross-platform conversation continuity
- Voice support (planned)

### 🛠 Tools
- **Browser**: Web search and site browsing
- **Shell**: Execute commands on the host
- **File**: Read, write, edit files
- **Vision**: Analyze images via **Gemini CLI** directly
- **Memory**: Store and search persistent memories
- **Cron**: Schedule automated tasks

### 📊 OpenHumans Integration
- Data source management
- Project creation and collaboration
- Member management
- Activity feeds

### 🎨 Live Canvas
- Agent-driven visual workspace
- Render text, charts, and HTML
- Real-time updates

### 🔒 Security
- DM pairing (unknown senders get a code)
- Allowlist-based access control
- Secure defaults

### ✨ Onboarding
- Interactive wizard (`openassistant onboard`)
- Guided setup of providers, channels, and skills
- Works on **macOS, Linux, and Windows**

## Quick Start

```bash
# Install
cargo install open-assistant

# Run onboarding wizard
openassistant onboard

# Start chatting
openassistant chat

# Start the gateway
openassistant gateway

# Check status
openassistant status

# Run diagnostics
openassistant doctor
```

## Commands

| Command | Description |
|---------|-------------|
| `chat` | Interactive CLI chat |
| `gateway` | Start messaging gateway |
| `onboard` | Run setup wizard |
| `config` | View/edit configuration |
| `status` | Show status |
| `doctor` | Run diagnostics |
| `update` | Update to latest version |

## Configuration

Configuration is stored in `~/.openassistant/config.yaml`:

```yaml
general:
  data_dir: ~/.openassistant
  log_level: info

model:
  provider: openrouter
  model: openrouter/owl-alpha
  api_key: your-api-key
  api_base: https://openrouter.ai/api/v1

gateway:
  discord_token: ""
  telegram_token: ""
  slack_token: ""
  webhook_port: 3000
  dm_policy: pairing

memory:
  db_path: ~/.openassistant/memory.db
  fts_enabled: true

skills:
  dirs:
    - ~/.openassistant/skills
  auto_create: true

security:
  dm_pairing: true
  allow_from: []

vision:
  provider: gemini-cli
  gemini_path: gemini
```

## Architecture

```
src/
├── main.rs           # CLI entry point
├── core/             # Agent engine, sessions, context
├── gateway/          # Multi-platform messaging
├── memory/           # Persistent memory (SQLite + FTS5)
├── skills/           # Skills system with auto-creation
├── cron/             # Scheduled automations
├── tools/            # Browser, shell, file, vision
├── platforms/        # OpenHumans data sources
├── canvas/           # Live canvas renderer
├── security/         # DM pairing, allowlist
├── onboarding/       # Setup wizard
├── ui/               # TUI chat interface
└── config/           # Configuration management
```

## Vision Tool

openAssistant uses **Gemini CLI directly** for vision analysis — no API key needed, just install Gemini CLI:

```bash
# Install Gemini CLI
npm install -g @anthropic/gemini-cli

# Or on Windows
iex (irm https://raw.githubusercontent.com/google/gemini-cli/main/scripts/install.ps1)

# Then the vision tool works automatically
open assistant → "Analyze this image: /path/to/image.jpg"
```

## License

MIT
