// src/ui/tui.rs
//! Terminal UI using ratatui — interactive chat interface
//! Features: message history, input box, status bar, tool output, markdown rendering

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

// ─── TUI Application ──────────────────────────────────────────────────

pub struct TuiApp {
    pub state: AppState,
    pub scroll_offset: usize,
    pub input_mode: InputMode,
    pub show_help: bool,
    pub show_sidebar: bool,
    pub message_list_state: ListState,
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
        }
    }
}

impl TuiApp {
    pub fn new() -> Self {
        let mut app = Self::default();
        app.state.add_message("system", "🦉 openAssistant v0.1.0 — Interactive TUI\nType /help for commands, Enter to send, Esc to quit.");
        app
    }

    /// Run the TUI event loop
    pub async fn run(&mut self) -> Result<()> {
        // Setup terminal
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        info!("TUI started");

        let result = self.event_loop(&mut terminal).await;

        // Cleanup
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()?;

        result
    }

    async fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        loop {
            // Draw
            terminal.draw(|f| self.draw(f))?;

            // Handle events
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
                                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                    break;
                                }
                                KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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

        // Add user message
        self.state.add_message("user", &input);
        self.state.input_buffer.clear();
        self.state.is_processing = true;
        self.state.update_status("Processing...");

        // In a full implementation, this would call the agent
        // For now, simulate a response
        let response = format!(
            "I received your message: \"{}\"\n\nIn a full implementation, this would call the agent loop with:\n- System prompt assembly (Step 4)\n- API call (Step 5)\n- Tool evaluation (Step 7)\n- Execution & observation (Steps 8-9)\n- Result (Step 11)",
            &input[..input.len().min(100)]
        );

        self.state.add_message("assistant", &response);
        self.state.is_processing = false;
        self.state.update_status("Ready");

        // Auto-scroll to bottom
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
            "/quit" | "/exit" => {
                // Will be handled by the event loop
            }
            "/status" => {
                let (completed, total) = (self.state.messages.len(), self.state.messages.len());
                self.state.update_status(&format!(
                    "Messages: {} | Model: {} | Mode: {} | Tokens: {}/{} | Cost: ${:.4}",
                    completed, self.state.model_name, self.state.permission_mode,
                    self.state.total_input_tokens, self.state.total_output_tokens,
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
                    self.state.update_status(&format!("Permission mode: {}", mode));
                } else {
                    self.state.add_message("system", &format!("Unknown command: {}. Type /help for available commands.", cmd));
                }
            }
        }

        Ok(())
    }

    // ─── Drawing ───────────────────────────────────────────────────────

    fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();

        // Main layout: sidebar + content
        let main_chunks = if self.show_sidebar {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(25), Constraint::Percentage(75)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(100)])
                .split(area)
        };

        let content_area = if self.show_sidebar { main_chunks[1] } else { main_chunks[0] };

        // Content layout: messages + input + status
        let content_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(3),                                    // Messages
                Constraint::Length(3),                                 // Input
                Constraint::Length(1),                                 // Status bar
            ])
            .split(content_area);

        // Draw sidebar
        if self.show_sidebar {
            self.draw_sidebar(frame, main_chunks[0]);
        }

        // Draw messages
        self.draw_messages(frame, content_chunks[0]);

        // Draw input
        self.draw_input(frame, content_chunks[1]);

        // Draw status bar
        self.draw_status_bar(frame, content_chunks[2]);

        // Draw help overlay
        if self.show_help {
            self.draw_help_overlay(frame, area);
        }
    }

    fn draw_sidebar(&self, frame: &mut Frame, area: Rect) {
        let sidebar = Block::default()
            .title("🦉 openAssistant")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = inner_rect(area);

        let msg_count = self.state.messages.len().to_string();
        let token_info = format!("{} in / {} out", self.state.total_input_tokens, self.state.total_output_tokens);
        let cost_info = format!("${:.4}", self.state.total_cost);

        let info = vec![
            Line::from(vec![Span::styled("Model:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&self.state.model_name)]),
            Line::from(""),
            Line::from(vec![Span::styled("Mode:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&self.state.permission_mode)]),
            Line::from(""),
            Line::from(vec![Span::styled("Workspace:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&self.state.workspace_dir)]),
            Line::from(""),
            Line::from(vec![Span::styled("Messages:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&msg_count)]),
            Line::from(""),
            Line::from(vec![Span::styled("Tokens:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&token_info)]),
            Line::from(""),
            Line::from(vec![Span::styled("Cost:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw(&cost_info)]),
            Line::from(""),
            Line::from(vec![Span::styled("Shortcuts:", Style::default().fg(Color::DarkGray))]),
            Line::from(vec![Span::raw("F1: Help")]   ),
            Line::from(vec![Span::raw("F2: Sidebar")]),
            Line::from(vec![Span::raw("Ctrl+L: Clear")]),
            Line::from(vec![Span::raw("Ctrl+C: Quit")]),
            Line::from(vec![Span::raw("PgUp/PgDn: Scroll")]),
        ];

        let paragraph = Paragraph::new(Text::from(info))
            .block(sidebar)
            .wrap(Wrap { trim: true });

        frame.render_widget(paragraph, area);
    }

    fn draw_messages(&mut self, frame: &mut Frame, area: Rect) {
        let messages_block = Block::default()
            .title(format!("💬 Messages ({})", self.state.messages.len()))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green));

        let items: Vec<ListItem> = self
            .state
            .messages
            .iter()
            .map(|msg| {
                let (icon, color) = match msg.role.as_str() {
                    "user" => ("👤", Color::Cyan),
                    "assistant" => ("🦉", Color::Green),
                    "system" => ("⚙️", Color::Yellow),
                    "tool" => ("🔧", Color::Magenta),
                    _ => ("💬", Color::White),
                };

                let content = if msg.content.len() > 200 {
                    msg.content[..200].to_string() + "..."
                } else {
                    msg.content.clone()
                };

                let header = format!("{} [{}] {}", icon, &msg.id[..6], msg.role);
                let line1 = Line::from(Span::styled(header, Style::default().fg(color)));
                let line2 = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(content, Style::default().fg(Color::White)),
                ]);
                let line3 = Line::from("");

                ListItem::new(Text::from(vec![line1, line2, line3]))
                    .style(Style::default())
            })
            .collect();

        let list = List::new(items)
            .block(messages_block)
            .highlight_style(Style::default().bg(Color::DarkGray))
            .scroll_padding(1);

        frame.render_stateful_widget(list, area, &mut self.message_list_state);
    }

    fn draw_input(&self, frame: &mut Frame, area: Rect) {
        let input_block = Block::default()
            .title(match self.input_mode {
                InputMode::Editing => "✏️  Input (Enter to send, / for commands)",
                InputMode::Command => "⌨️  Command (Enter to execute, Esc to cancel)",
                InputMode::Normal => "📝 Press 'i' to start typing",
            })
            .borders(Borders::ALL)
            .border_style(match self.input_mode {
                InputMode::Editing => Style::default().fg(Color::Cyan),
                InputMode::Command => Style::default().fg(Color::Yellow),
                InputMode::Normal => Style::default().fg(Color::DarkGray),
            });

        let input = Paragraph::new(self.state.input_buffer.as_str())
            .block(input_block)
            .style(Style::default().fg(Color::White));

        frame.render_widget(input, area);
    }

    fn draw_status_bar(&self, frame: &mut Frame, area: Rect) {
        let status_color = if self.state.is_processing {
            Color::Yellow
        } else {
            Color::Green
        };

        let status = Paragraph::new(Line::from(vec![
            Span::styled(
                if self.state.is_processing { "⏳ " } else { "✅ " },
                Style::default().fg(status_color),
            ),
            Span::raw(&self.state.status_message),
        ]))
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

        frame.render_widget(status, area);
    }

    fn draw_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let overlay = Rect::new(
            area.x + area.width / 6,
            area.y + area.height / 6,
            area.width * 2 / 3,
            area.height * 2 / 3,
        );

        let help_text = vec![
            Line::from(vec![Span::styled("🦉 openAssistant Help", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))]),
            Line::from(""),
            Line::from(vec![Span::styled("Commands:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw("  /help          Show this help")]),
            Line::from(vec![Span::raw("  /clear         Clear conversation")]),
            Line::from(vec![Span::raw("  /status        Show status")]),
            Line::from(vec![Span::raw("  /model <name>  Change model")]),
            Line::from(vec![Span::raw("  /mode <mode>   Change permission mode")]),
            Line::from(vec![Span::raw("  /quit          Exit")]),
            Line::from(""),
            Line::from(vec![Span::styled("Keyboard:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw("  Enter          Send message")]),
            Line::from(vec![Span::raw("  Esc            Close overlay / Quit")]),
            Line::from(vec![Span::raw("  Ctrl+C         Force quit")]),
            Line::from(vec![Span::raw("  Ctrl+L         Clear messages")]),
            Line::from(vec![Span::raw("  F1             Toggle help")]),
            Line::from(vec![Span::raw("  F2             Toggle sidebar")]),
            Line::from(vec![Span::raw("  PgUp/PgDn      Scroll messages")]),
            Line::from(""),
            Line::from(vec![Span::styled("Permission Modes:", Style::default().fg(Color::Yellow))]),
            Line::from(vec![Span::raw("  Default        Ask before all actions")]),
            Line::from(vec![Span::raw("  AcceptEdits    Auto-approve file writes")]),
            Line::from(vec![Span::raw("  Auto           Classifier-based approval")]),
            Line::from(vec![Span::raw("  Bypass         No prompts (dangerous!)")]),
        ];

        let help = Paragraph::new(Text::from(help_text))
            .block(
                Block::default()
                    .title("Help")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(Color::White).bg(Color::Black));

        frame.render_widget(Clear, overlay);
        frame.render_widget(help, overlay);
    }
}

fn inner_rect(rect: Rect) -> Rect {
    rect.inner(Margin::new(1, 1))
}

/// Entry point for TUI mode
pub async fn run_tui() -> Result<()> {
    let mut app = TuiApp::new();
    app.run().await
}
