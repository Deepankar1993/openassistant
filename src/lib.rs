// src/lib.rs
//! openAssistant core library.
//!
//! Exposes the agent engine and supporting modules as a reusable library so
//! that multiple front-ends can link the core in-process:
//! - the existing `clap` CLI binary (`src/main.rs`), and
//! - the Tauri desktop app (`src-tauri/`), which calls `core::agent::Agent`
//!   directly rather than going through the CLI.
//!
//! Module set is kept in sync with the declarations previously held in
//! `main.rs`. Internal modules reference one another via `crate::…`, which
//! now resolves to this library crate root.

pub mod core;
pub mod gateway;
pub mod memory;
pub mod skills;
pub mod cron;
pub mod tools;
pub mod platforms;
pub mod canvas;
pub mod security;
pub mod onboarding;
pub mod ui;
pub mod config;
