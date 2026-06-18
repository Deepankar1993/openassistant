// src/gateway/attachments.rs
//! Discord attachment ingest (Hermes parity).
//!
//! When a Discord user attaches text-like files to a message, we download the
//! accepted ones, extract their (UTF-8-lossy) text, and fold it into the
//! conversation content before the agent processes the message.
//!
//! Scope: plain-text files plus PDFs (text extracted via the pure-Rust
//! `pdf-extract` crate). Images/other binaries are rejected silently. Each
//! attachment is capped at [`MAX_ATTACHMENT_BYTES`] of extracted text and at
//! most [`MAX_ATTACHMENTS`] are read per message.

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
/// Raw-download ceiling for PDFs specifically. PDFs carry binary structure,
/// fonts and images, so 512 KiB is too tight for typical real documents; we
/// allow a modestly larger raw download here while still keeping the *extracted
/// text* bounded by [`MAX_ATTACHMENT_BYTES`]. Kept sane (4 MiB) so a single
/// attachment can't balloon memory.
pub const MAX_PDF_DOWNLOAD_BYTES: u64 = 4 * 1024 * 1024;
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

/// Decide whether an attachment is a PDF we should ingest. Accept by MIME
/// (`application/pdf`) first, then fall back to the `.pdf` extension.
pub fn is_pdf_attachment(filename: &str, content_type: Option<&str>) -> bool {
    if let Some(ct) = content_type {
        let mime = ct.split(';').next().unwrap_or("").trim().to_lowercase();
        if mime == "application/pdf" {
            return true;
        }
    }
    filename
        .rsplit_once('.')
        .map(|(_, e)| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

/// Extract text from raw PDF `bytes` using the pure-Rust `pdf-extract` crate.
///
/// Pure function over `&[u8]` so it is testable without a network or sample
/// file. Errors (corrupt/encrypted/unsupported PDFs) are propagated; image-only
/// PDFs legitimately yield an empty string. CPU-heavy and blocking — callers
/// run it inside `spawn_blocking`.
fn extract_pdf_text(bytes: &[u8]) -> anyhow::Result<String> {
    pdf_extract::extract_text_from_mem(bytes)
        .map_err(|e| anyhow::anyhow!("PDF text extraction failed: {e}"))
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

/// Download accepted text-like and PDF attachments from `msg` and return the
/// combined extracted text (empty when there are none). Network downloads happen
/// here, so this is not unit-tested — the pure helpers above are.
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

        // Classify the attachment and pick the applicable raw-download ceiling.
        let ct = att.content_type.as_deref();
        let is_pdf = is_pdf_attachment(&att.filename, ct);
        let is_text = !is_pdf && is_text_attachment(&att.filename, ct);
        if !is_pdf && !is_text {
            continue;
        }
        let size_cap = if is_pdf {
            MAX_PDF_DOWNLOAD_BYTES
        } else {
            MAX_DOWNLOAD_BYTES
        };

        // Reject oversized files by their declared size before buffering them
        // into memory (download() reads the whole body).
        if u64::from(att.size) > size_cap {
            warn!(
                "Discord attachment {} ({} bytes) exceeds the {} byte cap; skipping",
                att.filename, att.size, size_cap
            );
            continue;
        }
        let dl = tokio::time::timeout(
            std::time::Duration::from_secs(DOWNLOAD_TIMEOUT_SECS),
            att.download(),
        )
        .await;
        let bytes = match dl {
            Ok(Ok(bytes)) => bytes,
            Ok(Err(e)) => {
                warn!("Discord attachment download failed ({}): {}", att.filename, e);
                continue;
            }
            Err(_) => {
                warn!("Discord attachment download timed out ({})", att.filename);
                continue;
            }
        };

        // Extract text per type.
        let extracted = if is_pdf {
            // PDF extraction is CPU-heavy and blocking; run it off the async
            // executor. Errors (corrupt/encrypted) and empty output (image-only
            // PDFs) are handled gracefully — never panic.
            match tokio::task::spawn_blocking(move || extract_pdf_text(&bytes)).await {
                Ok(Ok(text)) => text,
                Ok(Err(e)) => {
                    warn!("Discord PDF extraction failed ({}): {}", att.filename, e);
                    continue;
                }
                Err(e) => {
                    warn!("Discord PDF extraction task panicked ({}): {}", att.filename, e);
                    continue;
                }
            }
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };

        if extracted.trim().is_empty() {
            // No usable text (e.g. an image-only/scanned PDF). Skip silently so
            // we don't emit an empty attachment block.
            if is_pdf {
                warn!("Discord PDF yielded no extractable text ({})", att.filename);
            }
            continue;
        }

        let capped = cap_text(&extracted, MAX_ATTACHMENT_BYTES);
        out.push_str(&format_attachment(&att.filename, &capped));
        read += 1;
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
        // PDFs are not "text" — they go through the dedicated PDF path instead.
        assert!(!is_text_attachment("report.pdf", Some("application/pdf")));
        assert!(!is_text_attachment("report.pdf", None));
        assert!(!is_text_attachment("noextension", None));
        assert!(!is_text_attachment("archive.zip", None));
    }

    #[test]
    fn is_pdf_attachment_accepts_pdf() {
        // By MIME.
        assert!(is_pdf_attachment("blob", Some("application/pdf")));
        assert!(is_pdf_attachment("blob", Some("application/pdf; qs=0.001")));
        // By extension (case-insensitive).
        assert!(is_pdf_attachment("report.pdf", None));
        assert!(is_pdf_attachment("REPORT.PDF", None));
        // MIME wins even if the extension is missing.
        assert!(is_pdf_attachment("statement", Some("application/pdf")));
    }

    #[test]
    fn is_pdf_attachment_rejects_non_pdf() {
        assert!(!is_pdf_attachment("notes.md", Some("text/plain")));
        assert!(!is_pdf_attachment("notes.md", None));
        assert!(!is_pdf_attachment("data.json", Some("application/json")));
        assert!(!is_pdf_attachment("photo.png", Some("image/png")));
        assert!(!is_pdf_attachment("photo.png", None));
        assert!(!is_pdf_attachment("noextension", None));
        // A ".pdf" substring that isn't the extension must not match.
        assert!(!is_pdf_attachment("mypdf.txt", None));
    }

    #[test]
    fn extract_pdf_text_on_garbage_errors_without_panic() {
        // Obviously-invalid bytes must return an Err rather than panicking.
        let res = extract_pdf_text(b"not a pdf at all");
        assert!(res.is_err());
    }

    #[test]
    fn extract_pdf_text_on_empty_errors_without_panic() {
        let res = extract_pdf_text(&[]);
        assert!(res.is_err());
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
