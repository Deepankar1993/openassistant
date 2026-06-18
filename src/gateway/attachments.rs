// src/gateway/attachments.rs
//! Discord attachment ingest (Hermes parity).
//!
//! When a Discord user attaches text-like files to a message, we download the
//! accepted ones, extract their (UTF-8-lossy) text, and fold it into the
//! conversation content before the agent processes the message.
//!
//! MVP scope: plain text only, no new crate dependencies. PDFs/images/binaries
//! are rejected silently. Each attachment is capped at [`MAX_ATTACHMENT_BYTES`]
//! of extracted text and at most [`MAX_ATTACHMENTS`] are read per message.

use tracing::warn;

/// Per-attachment cap on extracted text (truncated with a notice past this).
pub const MAX_ATTACHMENT_BYTES: usize = 64 * 1024;
/// Maximum number of attachments read from a single message (extras ignored).
pub const MAX_ATTACHMENTS: usize = 4;
/// Raw-download size ceiling. `Attachment::download()` buffers the whole file
/// into memory, so we reject anything larger than this via `att.size` *before*
/// downloading (Discord allows multi-MB uploads). Generous vs. the 64 KiB text
/// cap to allow whitespace/markup overhead.
pub const MAX_DOWNLOAD_BYTES: u64 = 512 * 1024;
/// Per-attachment download timeout, so a slow CDN can't stall the turn (and the
/// 👀→✅/❌ reaction) indefinitely.
const DOWNLOAD_TIMEOUT_SECS: u64 = 15;

/// Filename extensions accepted as text when the MIME type is missing or not
/// obviously text.
const TEXT_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "json", "yaml", "yml", "toml", "csv", "tsv", "rs",
    "py", "js", "ts", "go", "java", "c", "cpp", "h", "sh", "html", "css", "xml",
    "log", "ini", "cfg", "conf",
];

/// Decide whether an attachment is a text-like file we should ingest.
///
/// Accept by MIME (`text/*`, `application/json`) first, then fall back to a
/// filename-extension allowlist. Everything else (images, PDFs, binaries) is
/// rejected.
fn is_text_attachment(filename: &str, content_type: Option<&str>) -> bool {
    if let Some(ct) = content_type {
        // The MIME may carry a charset suffix (e.g. "text/plain; charset=utf-8").
        let mime = ct.split(';').next().unwrap_or("").trim().to_lowercase();
        if mime.starts_with("text/") || mime == "application/json" {
            return true;
        }
    }
    let ext = filename
        .rsplit_once('.')
        .map(|(_, e)| e.to_lowercase())
        .unwrap_or_default();
    !ext.is_empty() && TEXT_EXTENSIONS.contains(&ext.as_str())
}

/// Cap `body` to at most `max` bytes, truncating on a UTF-8 char boundary and
/// appending a `…[truncated]` notice when content was cut.
fn cap_text(body: &str, max: usize) -> String {
    if body.len() <= max {
        return body.to_string();
    }
    // Walk back to the nearest char boundary at or below `max`.
    let mut end = max;
    while end > 0 && !body.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n…[truncated]", &body[..end])
}

/// Render one attachment block: a labelled header line followed by its text.
fn format_attachment(filename: &str, body: &str) -> String {
    format!("\n\n[Attachment: {}]\n{}", filename, body)
}

/// Download accepted text-like attachments from `msg` and return the combined
/// extracted text (empty when there are none). Network downloads happen here, so
/// this is not unit-tested — the pure helpers above are.
pub async fn extract_attachments(msg: &serenity::all::Message) -> String {
    if msg.attachments.is_empty() {
        return String::new();
    }

    let mut out = String::new();
    let mut read = 0usize;
    let mut skipped_overflow = 0usize;

    for att in &msg.attachments {
        if read >= MAX_ATTACHMENTS {
            skipped_overflow += 1;
            continue;
        }
        if !is_text_attachment(&att.filename, att.content_type.as_deref()) {
            continue;
        }
        // Reject oversized files by their declared size before buffering them
        // into memory (download() reads the whole body).
        if u64::from(att.size) > MAX_DOWNLOAD_BYTES {
            warn!(
                "Discord attachment {} ({} bytes) exceeds the {} byte cap; skipping",
                att.filename, att.size, MAX_DOWNLOAD_BYTES
            );
            continue;
        }
        let dl = tokio::time::timeout(
            std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            att.download(),
        )
        .await;
        match dl {
            Ok(Ok(bytes)) => {
                let text = String::from_utf8_lossy(&bytes);
                let capped = cap_text(&text, MAX_ATTACHMENT_BYTES);
                out.push_str(&format_attachment(&att.filename, &capped));
                read += 1;
            }
            Ok(Err(e)) => {
                warn!("Discord attachment download failed ({}): {}", att.filename, e);
            }
            Err(_) => {
                warn!("Discord attachment download timed out ({})", att.filename);
            }
        }
    }

    if skipped_overflow > 0 {
        out.push_str(&format!(
            "\n\n[Note: {} additional attachment(s) skipped (max {} per message).]",
            skipped_overflow, MAX_ATTACHMENTS
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_text_attachment_accepts_text() {
        // By extension.
        assert!(is_text_attachment("notes.md", None));
        assert!(is_text_attachment("data.JSON", None));
        assert!(is_text_attachment("main.rs", None));
        // By MIME.
        assert!(is_text_attachment("blob", Some("text/plain")));
        assert!(is_text_attachment("blob", Some("text/plain; charset=utf-8")));
        assert!(is_text_attachment("blob", Some("application/json")));
    }

    #[test]
    fn is_text_attachment_rejects_binary() {
        assert!(!is_text_attachment("photo.png", Some("image/png")));
        assert!(!is_text_attachment("photo.png", None));
        assert!(!is_text_attachment("report.pdf", Some("application/pdf")));
        assert!(!is_text_attachment("report.pdf", None));
        assert!(!is_text_attachment("noextension", None));
        assert!(!is_text_attachment("archive.zip", None));
    }

    #[test]
    fn cap_text_leaves_short_unchanged() {
        let s = "hello world";
        assert_eq!(cap_text(s, 64), s);
    }

    #[test]
    fn cap_text_caps_long_input() {
        let s = "a".repeat(100);
        let out = cap_text(&s, 10);
        assert!(out.starts_with(&"a".repeat(10)));
        assert!(out.ends_with("…[truncated]"));
        assert!(out.len() < s.len() + 20);
    }

    #[test]
    fn cap_text_is_char_boundary_safe() {
        // Multibyte chars ("é" is 2 bytes). Capping mid-char must not panic and
        // must produce valid UTF-8.
        let s = "é".repeat(50); // 100 bytes
        let out = cap_text(&s, 5); // 5 is mid-char (boundaries at even offsets)
        assert!(out.ends_with("…[truncated]"));
        // The retained prefix must be whole "é" chars only.
        let prefix = out.trim_end_matches("…[truncated]").trim_end();
        assert!(prefix.chars().all(|c| c == 'é'));
    }

    #[test]
    fn format_attachment_has_prefix() {
        let out = format_attachment("notes.md", "body text");
        assert!(out.contains("[Attachment: notes.md]"));
        assert!(out.contains("body text"));
        assert!(out.starts_with("\n\n[Attachment: notes.md]\n"));
    }
}
