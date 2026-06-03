//! Shared application state for the desktop app.
//!
//! A single `tauri::async_runtime::Mutex` (a re-export of `tokio::sync::Mutex`,
//! whose guard is `Send` and therefore safe to hold across `.await` inside an
//! async command) guards the whole conversational turn. One lock per turn
//! prevents interleaved session writes / duplicated daily-note side effects if
//! the user submits twice quickly. See openspec change `add-desktop-app`,
//! tasks 2.1–2.2.

use open_assistant::core::agent::Agent;
use open_assistant::core::persona::{FullContext, Persona};
use open_assistant::core::session::Session;
use tauri::async_runtime::Mutex;

/// Everything mutated by a single conversational turn, behind one lock.
pub struct Turn {
    pub agent: Agent,
    pub ctx: FullContext,
    pub session: Session,
}

/// Top-level managed state registered via `app.manage(...)`.
pub struct AppCore {
    pub turn: Mutex<Turn>,
}

impl AppCore {
    pub fn new(agent: Agent, persona: Persona) -> Self {
        let mut ctx = FullContext::new();
        ctx.persona = persona; // injected into every system prompt
        Self {
            turn: Mutex::new(Turn {
                agent,
                ctx,
                session: Session::new("desktop", "local"),
            }),
        }
    }
}
