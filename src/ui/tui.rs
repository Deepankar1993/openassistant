// src/ui/tui.rs
//! Terminal UI using ratatui — modern interactive chat interface
//! Design inspired by OpenHumans: clean, card-like, with visual hierarchy
//!
//! Layout:
//! ┌-------------------------------------------------------------┐
//! │ 🦉 openAssistant v0.1.0          Model: owl-alpha  Mode: … │  ← Header
//! ├------------┬------------------------------------------------┤
//! │            │                                                │
//! │  Session   │  ┌------------------------------------------┐  │
//! │  ------    │  │ 🦉  openAssistant                       │  │
//! │  Model     │  │  Welcome to openAssistant…               │  │
//! │  Mode      │  └------------------------------------------┘  │
//! │  Tokens    │                                                │
//! │  Cost      │  ┌------------------------------------------┐  │
//! │            │  │ 👤  You                                  │  │
//! │  ------    │  │  Hello, can you help me?                │  │
//! │  Shortcuts │  └------------------------------------------┘  │
//! │  F1 Help   │                                                │
//! │  F2 Toggle │  ┌------------------------------------------┐  │
//! │            │  │ 🦉  openAssistant                       │  │
//! │            │  │  Of course! I'd be happy to help…       │  │
//! │            │  └------------------------------------------┘  │
//! ├------------┴------------------------------------------------┤
//! │ ✏  Type a message…                                    /help│  ← Input
//! ├-------------------------------------------------------------┤
//! │ ● Ready  │  Messages: 4  │  Tokens: 0/0  │  Cost: $0.0000 │  ← Status
//! └-------------------------------------------------------------┘

use crate::ui::AppState;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span, Text},
    widgets::{
        Block, Borders, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
        ScrollbarOrientation, Wrap,
    },
    Frame, Terminal,
};
use std::io;
use std::time::Duration;
use tracing::info;

// --- Color Palette (OpenHumans-inspired) ---------------------------
// Clean, modern terminal colors that mirror the web design
mod palette {
    use ratatui::style::Color;

    // Primary blues
    pub const PRIMARY: Color = Color::Rgb(37, 99, 235);       // #2563eb
    pub const PRIMARY_DIM: Color = Color::Rgb(29, 78, 216);   // #1d4ed8
    pub const PRIMARY_LIGHT: Color = Color::Rgb(219, 234, 254); // #dbeafe

    // Accent amber/orange
    pub const ACCENT: Color = Color::Rgb(245, 158, 11);      // #f59e0b
    pub const ACCENT_DIM: Color = Color::Rgb(217, 119, 6);    // #d97706

    // Neutrals (light theme adapted for terminal)
    pub const BG: Color = Color::Rgb(248, 250, 252);          // #f8fafc
    pub const BG_CARD: Color = Color::Rgb(255, 255, 255);     // #ffffff
    pub const BG_SIDEBAR: Color = Color::Rgb(241, 245, 249);  // #f1f5f9
    pub const BG_HEADER: Color = Color::Rgb(15, 23, 42);      // #0f172a

    pub const TEXT_PRIMARY: Color = Color::Rgb(15, 23, 42);    // #0f172a
    pub const TEXT_SECONDARY: Color = Color::Rgb(71, 85, 105); // #475569
    pub const TEXT_MUTED: Color = Color::Rgb(148, 163, 184);   // #94a3b8
    pub const TEXT_INVERSE: Color = Color::Rgb(255, 255, 255); // #ffffff

    // Semantic
    pub const SUCCESS: Color = Color::Rgb(16, 185, 129);      // #10b981
    pub const WARNING: Color = Color::Rgb(245, 158, 11);      // #f59e0b
    pub const DANGER: Color = Color::Rgb(239, 68, 68);        // #ef4444
    pub const INFO: Color = Color::Rgb(59, 130, 246);         // #3b82f6

    // Borders
    pub const BORDER: Color = Color::Rgb(226, 232, 240);      // #e2e8f0
    pub const BORDER_FOCUS: Color = Color::Rgb(37, 99, 235);   // #2563eb
}

// --- TUI Application --------------------------------------------------

pub struct TuiApp {
    pub state: AppState,
    pub scroll_offset: usize,
    pub input_mode: InputMode,
    pub show_help: bool,
    pub show_sidebar: bool,
    pub message_list_state: ListState,
    pub sidebar_scroll: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
    Command,
}

impl Default for TuiApp {
    fn default() -> Self {
        Self {
            state: AppState::default(),
            scroll_offset: 0,
            input_mode: InputMode::Editing,
            show_help: false,
            show_sidebar: true,
            message_list_state: ListState::default(),
            sidebar_scroll: 0,
        }
    }
}

impl TuiApp {
    pub fn new() -> Self {
        let mut app = Self::default();
        app.state.add_message(
            "system",
            "Welcome to openAssistant — Your AI companion with terminal access, web search, browser control, and self-management.",
        );
        app
    }

    /// Run the TUI event loop
    pub async fn run(&mut self) -> Result<()> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        info!("TUI started");

        let result = self.event_loop(&mut terminal).await;

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        loop {
            terminal.draw(|f| self.draw(f))?;

            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        match self.input_mode {
                            InputMode::Editing => match key.code {
                                KeyCode::Enter => {
                                    self.send_message().await?;
                                }
                                KeyCode::Esc => {
                                    if self.show_help {
                                        self.show_help = false;
                                    } else if self.show_sidebar {
                                        self.show_sidebar = false;
                                    } else {
                                        break;
                                    }
                                }
                                KeyCode::Char('c')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    break;
                                }
                                KeyCode::Char('l')
                                    if key.modifiers.contains(KeyModifiers::CONTROL) =>
                                {
                                    self.state.clear_messages();
                                }
                                KeyCode::Char('/') => {
                                    self.input_mode = InputMode::Command;
                                    self.state.input_buffer = "/".to_string();
                                }
                                KeyCode::Char(c) => {
                                    self.state.input_buffer.push(c);
                                }
                                KeyCode::Backspace => {
                                    self.state.input_buffer.pop();
                                }
                                KeyCode::PageUp => {
                                    self.scroll_offset = self.scroll_offset.saturating_add(5);
                                }
                                KeyCode::PageDown => {
                                    self.scroll_offset = self.scroll_offset.saturating_sub(5);
                                }
                                KeyCode::F(1) => {
                                    self.show_help = !self.show_help;
                                }
                                KeyCode::F(2) => {
                                    self.show_sidebar = !self.show_sidebar;
                                }
                                _ => {}
                            },
                            InputMode::Command => match key.code {
                                KeyCode::Enter => {
                                    self.execute_command().await?;
                                }
                                KeyCode::Esc => {
                                    self.input_mode = InputMode::Editing;
                                    self.state.input_buffer.clear();
                                }
                                KeyCode::Char(c) => {
                                    self.state.input_buffer.push(c);
                                }
                                KeyCode::Backspace => {
                                    if self.state.input_buffer.len() > 1 {
                                        self.state.input_buffer.pop();
                                    } else {
                                        self.input_mode = InputMode::Editing;
                                        self.state.input_buffer.clear();
                                    }
                                }
                                _ => {}
                            },
                            InputMode::Normal => match key.code {
                                KeyCode::Char('i') => {
                                    self.input_mode = InputMode::Editing;
                                }
                                KeyCode::Char('q') => break,
                                _ => {}
                            },
                        }
                    }
                }
            }
        }

        info!("TUI exited");
        Ok(())
    }

    async fn send_message(&mut self) -> Result<()> {
        let input = self.state.input_buffer.trim().to_string();
        if input.is_empty() {
            return Ok(());
        }

        self.state.add_message("user", &input);
        self.state.input_buffer.clear();
        self.state.is_processing = true;
        self.state.update_status("Processing...");

        let response = format!(
            "I received your message: \"{}\"\n\nIn a full implementation, this would call the 11-step ReAct agent loop with tool dispatch, memory search, and multi-source web search.",
            &input[..input.len().min(100)]
        );

        self.state.add_message("assistant", &response);
        self.state.is_processing = false;
        self.state.update_status("Ready");

        self.scroll_offset = 0;

        Ok(())
    }

    async fn execute_command(&mut self) -> Result<()> {
        let cmd = self.state.input_buffer.trim().to_string();
        self.state.input_buffer.clear();
        self.input_mode = InputMode::Editing;

        match cmd.as_str() {
            "/help" => {
                self.show_help = true;
            }
            "/clear" => {
                self.state.clear_messages();
            }
            "/quit" | "/exit" => {}
            "/status" => {
                self.state.update_status(&format!(
                    "Messages: {} | Model: {} | Mode: {} | Tokens: {}/{} | Cost: ${:.4}",
                    self.state.messages.len(),
                    self.state.model_name,
                    self.state.permission_mode,
                    self.state.total_input_tokens,
                    self.state.total_output_tokens,
                    self.state.total_cost
                ));
            }
            _ => {
                if cmd.starts_with("/model ") {
                    let model = cmd.trim_start_matches("/model ").trim();
                    if !model.is_empty() {
                        self.state.model_name = model.to_string();
                        self.state.update_status(&format!("Model changed to: {}", model));
                    }
                } else if cmd.starts_with("/mode ") {
                    let mode = cmd.trim_start_matches("/mode ").trim();
                    self.state.permission_mode = mode.to_string();
                    self.state
                        .update_status(&format!("Permission mode: {}", mode));
                } else {
                    self.state.add_message(
                        "system",
                        &format!(
                            "Unknown command: {}. Type /help for available commands.",
                            cmd
                        ),
                    );
                }
            }
        }

        Ok(())
    }

    // --- Drawing -------------------------------------------------------

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Root layout: header + body + input + status
        let root_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),   // Header bar
                Constraint::Min(3),      // Main content (sidebar + messages)
                Constraint::Length(3),   // Input area
                Constraint::Length(1),   // Status bar
            ])
            .split(area);

        // Draw header
        self.draw_header(frame, root_chunks[0]);

        // Main content area: sidebar + messages
        let content_area = root_chunks[1];
        let content_chunks = if self.show_sidebar {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(28),  // Sidebar (fixed width)
                    Constraint::Min(40),     // Messages
                ])
                .split(content_area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(content_area)
        };

        if self.show_sidebar {
            self.draw_sidebar(frame, content_chunks[0]);
        }

        let msg_area = if self.show_sidebar {
            content_chunks[1]
        } else {
            content_chunks[0]
        };

        self.draw_messages(frame, msg_area);

        // Draw input
        self.draw_input(frame, root_chunks[2]);

        // Draw status bar
        self.draw_status_bar(frame, root_chunks[3]);

        // Draw help overlay
        if self.show_help {
            self.draw_help_overlay(frame, area);
        }
    }

    // --- Header Bar ---------------------------------------------------

    fn draw_header(&self, frame: &mut Frame, area: Rect) {
        let model_short = if self.state.model_name.len() > 20 {
            format!("{}…", &self.state.model_name[..20])
        } else {
            self.state.model_name.clone()
        };

        let left = format!(" 🦉 openAssistant v0.1.0 ");
        let right = format!(" {} │ {} ", model_short, self.state.permission_mode);
        let total_width = left.len() + 3 + right.len();
        let padding = area.width as usize - total_width.min(area.width as usize);
        let padding_str = " ".repeat(padding);

        let header = Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().fg(palette::TEXT_INVERSE).bg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled(
                " │ ",
                Style::default().fg(palette::PRIMARY).bg(palette::BG_HEADER),
            ),
            Span::styled(
                &right,
                Style::default().fg(palette::TEXT_MUTED).bg(palette::BG_HEADER),
            ),
            Span::styled(
                padding_str,
                Style::default().bg(palette::BG_HEADER),
            ),
        ]))
        .style(Style::default().bg(palette::BG_HEADER));

        frame.render_widget(header, area);
    }

    // --- Sidebar ------------------------------------------------------

    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let inner = area.inner(Margin::new(1, 1));

        // Section: Session
        let session_title = Line::from(vec![
            Span::styled(" SESSION", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
        ]);

        let model_label = Line::from(vec![
            Span::styled("  Model", Style::default().fg(palette::TEXT_MUTED)),
        ]);
        let model_value = Line::from(vec![
            Span::styled(format!("  {}", self.state.model_name), Style::default().fg(palette::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
        ]);

        let mode_label = Line::from(vec![
            Span::styled("  Mode", Style::default().fg(palette::TEXT_MUTED)),
        ]);
        let mode_value = Line::from(vec![
            Span::styled(format!("  {}", self.state.permission_mode), Style::default().fg(palette::PRIMARY)),
        ]);

        // Section: Usage
        let usage_title = Line::from(vec![
            Span::styled(" USAGE", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
        ]);

        let msg_count = self.state.messages.len().to_string();
        let token_str = format!("{} / {}", self.state.total_input_tokens, self.state.total_output_tokens);
        let cost_str = format!("${:.4}", self.state.total_cost);

        let messages_line = Line::from(vec![
            Span::styled("  Messages  ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(&msg_count, Style::default().fg(palette::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
        ]);
        let tokens_line = Line::from(vec![
            Span::styled("  Tokens    ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(&token_str, Style::default().fg(palette::TEXT_PRIMARY)),
        ]);
        let cost_line = Line::from(vec![
            Span::styled("  Cost      ", Style::default().fg(palette::TEXT_MUTED)),
            Span::styled(&cost_str, Style::default().fg(palette::SUCCESS)),
        ]);

        // Section: Shortcuts
        let shortcuts_title = Line::from(vec![
            Span::styled(" SHORTCUTS", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
        ]);

        let shortcuts = vec![
            Line::from(vec![
                Span::styled("  F1 ", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("Help", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  F2 ", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("Sidebar", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  ⌃L ", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("Clear", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  ⌃C ", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("Quit", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  ⇟  ", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled("Scroll", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
        ];

        // Section: Capabilities
        let caps_title = Line::from(vec![
            Span::styled(" CAPABILITIES", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
        ]);

        let caps = vec![
            Line::from(vec![
                Span::styled("  🔍 Web Search", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  🌐 Browser", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  📁 Files", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  💻 Terminal", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  🤖 Sub-Agents", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
            Line::from(vec![
                Span::styled("  🔧 Tools", Style::default().fg(palette::TEXT_SECONDARY)),
            ]),
        ];

        // Assemble all sidebar content
        // Compute separator lines outside the vec![] macro to avoid
        // Rust tokenizer seeing ("-".repeat()) as a negative float literal
        let sep1 = "-".repeat(inner.width as usize);
        let sep2 = "-".repeat(inner.width as usize);
        let sep3 = "-".repeat(inner.width as usize);

        let mut all_lines: Vec<Line> = vec![
            session_title,
            Line::from(""),
            model_label,
            model_value,
            Line::from(""),
            mode_label,
            mode_value,
            Line::from(""),
            Line::from(vec![Span::styled(&sep1, Style::default().fg(palette::BORDER))]),
            usage_title,
            Line::from(""),
            messages_line,
            tokens_line,
            cost_line,
            Line::from(""),
            Line::from(vec![Span::styled(&sep2, Style::default().fg(palette::BORDER))]),
            shortcuts_title,
            Line::from(""),
        ];
        all_lines.extend(shortcuts);
        all_lines.push(Line::from(""));
        all_lines.push(Line::from(vec![Span::styled(&sep3, Style::default().fg(palette::BORDER))]));
        all_lines.push(caps_title);
        all_lines.push(Line::from(""));
        all_lines.extend(caps);

        let sidebar_block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(palette::BORDER))
            .style(Style::default().bg(palette::BG_SIDEBAR));

        let paragraph = Paragraph::new(Text::from(all_lines))
            .block(sidebar_block)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    // --- Messages Area ------------------------------------------------

    fn draw_messages(&mut self, frame: &mut Frame, area: Rect) {
        let inner = area.inner(Margin::new(1, 0));
        let msg_count = self.state.messages.len();

        // Title bar for messages area
        let title = format!(" Conversation — {} message{} ", msg_count, if msg_count == 1 { "" } else { "s" });

        let msg_block = Block::default()
            .title(title)
            .title_style(Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD))
            .borders(Borders::TOP)
            .border_style(Style::default().fg(palette::BORDER))
            .style(Style::default().bg(palette::BG));

        if self.state.messages.is_empty() {
            // Welcome / empty state
            let welcome = Paragraph::new(Text::from(vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled("  🦉 ", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD)),
                    Span::styled("Welcome to openAssistant", Style::default().fg(palette::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Your AI companion with terminal access, web search,", Style::default().fg(palette::TEXT_SECONDARY)),
                ]),
                Line::from(vec![
                    Span::styled("  browser control, sub-agents, and self-management.", Style::default().fg(palette::TEXT_SECONDARY)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Try:", Style::default().fg(palette::TEXT_MUTED)),
                ]),
                Line::from(vec![
                    Span::styled("  • \"Help me organize my project files\"", Style::default().fg(palette::TEXT_SECONDARY)),
                ]),
                Line::from(vec![
                    Span::styled("  • \"Search the web for latest AI news\"", Style::default().fg(palette::TEXT_SECONDARY)),
                ]),
                Line::from(vec![
                    Span::styled("  • \"Run system diagnostics\"", Style::default().fg(palette::TEXT_SECONDARY)),
                ]),
                Line::from(""),
                Line::from(vec![
                    Span::styled("  Press ", Style::default().fg(palette::TEXT_MUTED)),
                    Span::styled("/", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(" for commands, ", Style::default().fg(palette::TEXT_MUTED)),
                    Span::styled("F1", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                    Span::styled(" for help", Style::default().fg(palette::TEXT_MUTED)),
                ]),
            ]))
            .block(msg_block)
            .wrap(Wrap { trim: true });

            frame.render_widget(welcome, area);
            return;
        }

        // Render messages as styled cards
        let items: Vec<ListItem> = self
            .state
            .messages
            .iter()
            .map(|msg| {
                let (icon, role_color, bg_tint) = match msg.role.as_str() {
                    "user" => ("👤", palette::PRIMARY, palette::PRIMARY_LIGHT),
                    "assistant" => ("🦉", palette::SUCCESS, Color::Rgb(236, 253, 245)),
                    "system" => ("⚙ ", palette::WARNING, palette::BG_SIDEBAR),
                    "tool" => ("🔧", palette::INFO, Color::Rgb(239, 246, 255)),
                    _ => ("💬", palette::TEXT_MUTED, palette::BG),
                };

                // Truncate content for display
                let content = if msg.content.len() > 300 {
                    format!("{}…", &msg.content[..300])
                } else {
                    msg.content.clone()
                };

                // Role label
                let role_label = match msg.role.as_str() {
                    "user" => "You",
                    "assistant" => "openAssistant",
                    "system" => "System",
                    "tool" => "Tool",
                    _ => &msg.role,
                };

                let header = Line::from(vec![
                    Span::styled(
                        format!(" {}  ", icon),
                        Style::default().fg(role_color).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(
                        role_label,
                        Style::default().fg(role_color).add_modifier(Modifier::BOLD),
                    ),
                ]);

                // Content lines
                let content_lines: Vec<Line> = content
                    .lines()
                    .map(|line| {
                        Line::from(vec![
                            Span::styled("    ", Style::default()),
                            Span::styled(line.to_string(), Style::default().fg(palette::TEXT_PRIMARY)),
                        ])
                    })
                    .collect();

                let mut lines = vec![header, Line::from("")];
                lines.extend(content_lines);
                lines.push(Line::from(""));

                ListItem::new(Text::from(lines)).style(Style::default().bg(palette::BG))
            })
            .collect();

        let list = List::new(items)
            .block(msg_block)
            .highlight_style(Style::default().bg(palette::BG_SIDEBAR))
            .scroll_padding(1);

        frame.render_stateful_widget(list, area, &mut self.message_list_state);
    }

    // --- Input Area ---------------------------------------------------

    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        let (title, border_color) = match self.input_mode {
            InputMode::Editing => (
                " Message — Enter to send, / for commands ",
                palette::PRIMARY,
            ),
            InputMode::Command => (
                " Command — Enter to execute, Esc to cancel ",
                palette::ACCENT,
            ),
            InputMode::Normal => (
                " Press 'i' to start typing ",
                palette::TEXT_MUTED,
            ),
        };

        let hint = match self.input_mode {
            InputMode::Editing => " /help ",
            InputMode::Command => " esc to cancel ",
            InputMode::Normal => " i to edit ",
        };

        let input_block = Block::default()
            .title(title)
            .title_style(Style::default().fg(border_color).add_modifier(Modifier::BOLD))
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(palette::BG_CARD));

        let input_inner = area.inner(Margin::new(1, 1));

        let input_text = if self.state.input_buffer.is_empty() {
            match self.input_mode {
                InputMode::Editing => "Type a message…",
                InputMode::Command => "Enter command…",
                InputMode::Normal => "Press 'i' to type…",
            }
        } else {
            &self.state.input_buffer
        };

        let input_color = if self.state.input_buffer.is_empty() {
            palette::TEXT_MUTED
        } else {
            palette::TEXT_PRIMARY
        };

        let input = Paragraph::new(Span::styled(
            input_text,
            Style::default().fg(input_color),
        ))
        .block(input_block)
        .wrap(Wrap { trim: true });

        frame.render_widget(input, area);

        // Render hint on the right side
        let hint_area = Rect::new(
            area.x + area.width.saturating_sub(unicode_width::UnicodeWidthStr::width(hint) as u16 + 2),
            area.y + 1,
            unicode_width::UnicodeWidthStr::width(hint) as u16 + 1,
            1,
        );
        let hint_widget = Paragraph::new(Span::styled(
            hint,
            Style::default().fg(palette::TEXT_MUTED).bg(palette::BG_SIDEBAR),
        ));
        frame.render_widget(hint_widget, hint_area);
    }

    // --- Status Bar ---------------------------------------------------

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let (dot, dot_color) = if self.state.is_processing {
            ("●", palette::WARNING)
        } else {
            ("●", palette::SUCCESS)
        };

        let status_text = &self.state.status_message;
        let msg_count = self.state.messages.len();
        let token_str = format!("{} / {}", self.state.total_input_tokens, self.state.total_output_tokens);
        let cost_str = format!("${:.4}", self.state.total_cost);

        let bar = Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", dot),
                Style::default().fg(dot_color),
            ),
            Span::styled(
                format!(" {} │ ", status_text),
                Style::default().fg(palette::TEXT_SECONDARY),
            ),
            Span::styled(
                "Msgs: ", Style::default().fg(palette::TEXT_MUTED),
            ),
            Span::styled(
                format!("{} │ ", msg_count), Style::default().fg(palette::TEXT_PRIMARY),
            ),
            Span::styled(
                "Tokens: ", Style::default().fg(palette::TEXT_MUTED),
            ),
            Span::styled(
                format!("{} │ ", token_str), Style::default().fg(palette::TEXT_PRIMARY),
            ),
            Span::styled(
                "Cost: ", Style::default().fg(palette::TEXT_MUTED),
            ),
            Span::styled(
                &cost_str, Style::default().fg(palette::SUCCESS),
            ),
        ]))
        .style(Style::default().bg(palette::BG_HEADER).fg(palette::TEXT_INVERSE));

        frame.render_widget(bar, area);
    }

    // --- Help Overlay -------------------------------------------------

    fn draw_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let overlay_w = area.width * 3 / 4;
        let overlay_h = area.height * 3 / 4;
        let overlay = Rect::new(
            area.x + (area.width - overlay_w) / 2,
            area.y + (area.height - overlay_h) / 2,
            overlay_w,
            overlay_h,
        );

        let help_text = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  🦉 openAssistant Help", Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD | Modifier::UNDERLINED)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Commands", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("    /help", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("            Show this help", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    /clear", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("           Clear conversation", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    /status", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("          Show session status", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    /model <n>", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("       Change model", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    /mode <m>", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("        Change permission mode", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    /quit", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("            Exit", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Keyboard", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("    Enter", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("          Send message", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    Esc", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("            Close / Quit", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    Ctrl+C", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("        Force quit", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    Ctrl+L", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("        Clear messages", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    F1", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("              Toggle help", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    F2", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("              Toggle sidebar", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    PgUp/PgDn", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("     Scroll messages", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Permission Modes", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
            ]),
            Line::from(vec![
                Span::styled("    Default", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("        Ask before all actions", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    AcceptEdits", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("    Auto-approve file writes", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    Auto", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("           Classifier-based approval", Style::default().fg(palette::TEXT_MUTED)),
            ]),
            Line::from(vec![
                Span::styled("    Bypass", Style::default().fg(palette::TEXT_PRIMARY)),
                Span::styled("         No prompts (dangerous!)", Style::default().fg(palette::DANGER)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Press ", Style::default().fg(palette::TEXT_MUTED)),
                Span::styled("Esc", Style::default().fg(palette::ACCENT).add_modifier(Modifier::BOLD)),
                Span::styled(" to close", Style::default().fg(palette::TEXT_MUTED)),
            ]),
        ];

        let help = Paragraph::new(Text::from(help_text))
            .block(
                Block::default()
                    .title(" Help ")
                    .title_style(Style::default().fg(palette::PRIMARY).add_modifier(Modifier::BOLD))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette::PRIMARY))
                    .style(Style::default().bg(palette::BG_CARD)),
            )
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(palette::TEXT_PRIMARY).bg(palette::BG_CARD));

        frame.render_widget(Clear, overlay);
        frame.render_widget(help, overlay);
    }
}

/// Entry point for TUI mode
pub async fn run_tui() -> Result<()> {
    let mut app = TuiApp::new();
    app.run().await
}
