// src/core/proc.rs
//! Cross-platform subprocess helpers.
//!
//! On **Windows**, spawning a console subprocess (the `claude` bridge, `bash`,
//! `gemini`, `git`, an MCP server, a lifecycle hook, …) from the GUI desktop app
//! or the background gateway pops a visible console window — once per spawn,
//! which for the per-message Claude bridge means a window flashes on every chat
//! message. `no_window` / `no_window_std` set the `CREATE_NO_WINDOW` process
//! creation flag so those children stay invisible. Both are **no-ops on
//! non-Windows** platforms (the `#[cfg(windows)]` block compiles out, leaving an
//! identity function), so call sites are identical across platforms.

/// `CREATE_NO_WINDOW` (winbase.h) — the child runs without allocating a console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Suppress the console window for a `tokio::process::Command` on Windows.
/// Returns the same `&mut` so it chains in a builder statement.
pub fn no_window(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(windows)]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Suppress the console window for a `std::process::Command` on Windows
/// (used by the tools that run under `spawn_blocking`).
pub fn no_window_std(cmd: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_are_chainable_noops_off_windows() {
        // The point of the test is that both helpers compile and return the
        // command for chaining on every platform; on Windows they additionally
        // set the creation flag (not observable without spawning).
        let mut t = tokio::process::Command::new("true");
        let _ = no_window(&mut t);
        let mut s = std::process::Command::new("true");
        let _ = no_window_std(&mut s);
    }
}
