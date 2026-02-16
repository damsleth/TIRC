use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};

/// Log entry schema persisted as JSONL.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRecord {
    pub ts: DateTime<Utc>,
    pub network: String,
    pub buffer: String,
    pub sender: Option<String>,
    pub tags: std::collections::BTreeMap<String, String>,
    pub kind: String,
    pub body: String,
}

#[derive(Debug, Error)]
pub enum LogError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Append-only JSONL logger.
#[derive(Debug, Clone)]
pub struct LogWriter {
    root: PathBuf,
    retention_days: u16,
    redact_secrets: bool,
}

impl LogWriter {
    pub fn new(root: impl Into<PathBuf>, retention_days: u16, redact_secrets: bool) -> Self {
        Self {
            root: root.into(),
            retention_days,
            redact_secrets,
        }
    }

    pub async fn append(&self, record: &LogRecord) -> Result<PathBuf, LogError> {
        let record_to_write = if self.redact_secrets {
            let mut redacted = record.clone();
            redacted.body = redact_sensitive_content(&record.body);
            redacted
        } else {
            record.clone()
        };
        
        let date = record_to_write.ts.date_naive();
        let file = self
            .root
            .join(sanitize(&record_to_write.network))
            .join(sanitize(&record_to_write.buffer))
            .join(format!(
                "{:04}-{:02}-{:02}.jsonl",
                date.year(),
                date.month(),
                date.day()
            ));

        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).await?;
        }

        let line = serde_json::to_string(&record_to_write)?;
        let handle = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&file)
            .await?;
        let mut writer = BufWriter::new(handle);
        writer.write_all(line.as_bytes()).await?;
        writer.write_all(b"\n").await?;
        writer.flush().await?;

        Ok(file)
    }

    pub async fn cleanup_retention(&self, now: DateTime<Utc>) -> Result<(), LogError> {
        let cutoff = now - chrono::Duration::days(i64::from(self.retention_days));
        cleanup_dir(&self.root, cutoff.date_naive()).await
    }
}

async fn cleanup_dir(root: &Path, cutoff: chrono::NaiveDate) -> Result<(), LogError> {
    if !root.exists() {
        return Ok(());
    }

    let mut stacks = vec![root.to_path_buf()];
    while let Some(path) = stacks.pop() {
        let mut entries = fs::read_dir(&path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let p = entry.path();
            if p.is_dir() {
                stacks.push(p);
                continue;
            }

            let Some(name) = p.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };

            if let Ok(date) = chrono::NaiveDate::parse_from_str(name, "%Y-%m-%d")
                && date < cutoff
            {
                let _ = fs::remove_file(&p).await;
            }
        }
    }

    Ok(())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '#' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Redact sensitive content from log messages.
///
/// This function scans for known credential patterns and replaces
/// the secret portion with `[REDACTED]`. It processes **all** occurrences
/// of each pattern, not just the first.
///
/// Patterns redacted:
/// - NickServ / NS commands: `/msg nickserv identify <pw>`, `/ns identify <pw>`,
///   `/nickserv register <pw>`, etc. Only triggers when preceded by a recognised
///   service keyword to avoid false positives on normal chat containing "identify".
/// - `AUTHENTICATE <base64-payload>` (SASL) — any AUTHENTICATE line with a
///   non-empty payload, including continuation lines (400+ byte splits).
/// - `password=`, `password:`, `pass=`, `pass:` key-value patterns.
/// - `PASS <password>` IRC server password command.
fn redact_sensitive_content(body: &str) -> String {
    let lower = body.to_ascii_lowercase();

    // --- SASL AUTHENTICATE (entire payload is secret) ---
    if lower.starts_with("authenticate ") && body.len() > "authenticate ".len() {
        return "AUTHENTICATE [REDACTED]".to_owned();
    }

    // --- IRC PASS command ---
    if lower.starts_with("pass ") && body.len() > "pass ".len() {
        return "PASS [REDACTED]".to_owned();
    }

    // --- NickServ / NS identify / register ---
    // Only match when a recognised service keyword immediately precedes the
    // verb, so "I identify with that" in normal chat is *not* redacted.
    let service_patterns: &[&str] = &[
        "nickserv identify ",
        "nickserv register ",
        "nickserv set password ",
        "nickserv ghost ",
        "nickserv release ",
        "nickserv group ",
        "ns identify ",
        "ns register ",
        "ns set password ",
        "ns ghost ",
        "ns release ",
        "ns group ",
    ];

    for pattern in service_patterns {
        if let Some(pos) = lower.find(pattern) {
            let start = pos + pattern.len();
            return redact_token_at(body, start);
        }
    }

    // --- password/pass key-value patterns (handles all occurrences) ---
    let kv_patterns: &[&str] = &[
        "password=",
        "password:",
        "password ",
        "pass=",
        "pass:",
    ];

    let mut result = body.to_owned();
    let mut result_lower = lower;
    for pattern in kv_patterns {
        // Replace all occurrences of this pattern
        let mut search_from = 0;
        while let Some(pos) = result_lower[search_from..].find(pattern) {
            let abs_pos = search_from + pos;
            let start = abs_pos + pattern.len();
            if start >= result.len() {
                break;
            }
            let token_end = result[start..]
                .find(|c: char| c.is_whitespace() || c == '&' || c == ';' || c == '"' || c == '\'')
                .map_or(result.len(), |p| start + p);
            if token_end > start {
                let replacement = format!("{}[REDACTED]{}", &result[..start], &result[token_end..]);
                result_lower = replacement.to_ascii_lowercase();
                result = replacement;
                // Advance past the replacement marker so we don't re-match it
                search_from = start + "[REDACTED]".len();
            } else {
                search_from = start;
            }
        }
    }

    result
}

/// Replace the first whitespace-delimited token starting at `start` with `[REDACTED]`.
fn redact_token_at(body: &str, start: usize) -> String {
    let end = body[start..]
        .find(|c: char| c.is_whitespace())
        .map_or(body.len(), |p| start + p);
    if end <= start {
        return body.to_owned();
    }
    let mut out = body[..start].to_owned();
    out.push_str("[REDACTED]");
    if end < body.len() {
        out.push_str(&body[end..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redact_nickserv_identify() {
        assert_eq!(
            redact_sensitive_content("/msg nickserv identify mypassword"),
            "/msg nickserv identify [REDACTED]"
        );
    }

    #[test]
    fn test_redact_nickserv_identify_case_insensitive() {
        assert_eq!(
            redact_sensitive_content("/msg NickServ IDENTIFY MySecret"),
            "/msg NickServ IDENTIFY [REDACTED]"
        );
    }

    #[test]
    fn test_redact_ns_identify() {
        assert_eq!(
            redact_sensitive_content("/ns identify secretpass"),
            "/ns identify [REDACTED]"
        );
    }

    #[test]
    fn test_redact_nickserv_set_password() {
        assert_eq!(
            redact_sensitive_content("/msg nickserv set password newpass123"),
            "/msg nickserv set password [REDACTED]"
        );
    }

    #[test]
    fn test_redact_nickserv_ghost() {
        assert_eq!(
            redact_sensitive_content("/msg nickserv ghost oldnick mypass"),
            "/msg nickserv ghost [REDACTED] mypass"
        );
    }

    #[test]
    fn test_redact_authenticate() {
        assert_eq!(
            redact_sensitive_content("AUTHENTICATE AGJvdABteXNlY3JldA=="),
            "AUTHENTICATE [REDACTED]"
        );
    }

    #[test]
    fn test_redact_authenticate_long_payload() {
        // SASL splits payloads at 400 bytes; each chunk is its own AUTHENTICATE line
        let payload = "A".repeat(400);
        assert_eq!(
            redact_sensitive_content(&format!("AUTHENTICATE {}", payload)),
            "AUTHENTICATE [REDACTED]"
        );
    }

    #[test]
    fn test_redact_pass_command() {
        assert_eq!(
            redact_sensitive_content("PASS serverpassword"),
            "PASS [REDACTED]"
        );
    }

    #[test]
    fn test_no_redaction_normal_message() {
        let msg = "Hello everyone, how are you today?";
        assert_eq!(redact_sensitive_content(msg), msg);
    }

    #[test]
    fn test_no_false_positive_identify_in_chat() {
        // "identify" in normal chat should NOT trigger redaction
        let msg = "I identify with that sentiment completely";
        assert_eq!(redact_sensitive_content(msg), msg);
    }

    #[test]
    fn test_no_false_positive_identify_word() {
        let msg = "Can you identify the problem?";
        assert_eq!(redact_sensitive_content(msg), msg);
    }

    #[test]
    fn test_redact_password_equals() {
        assert_eq!(
            redact_sensitive_content("some config password=secret123 other"),
            "some config password=[REDACTED] other"
        );
    }

    #[test]
    fn test_redact_password_colon() {
        assert_eq!(
            redact_sensitive_content("password:hunter2"),
            "password:[REDACTED]"
        );
    }

    #[test]
    fn test_redact_pass_equals() {
        assert_eq!(
            redact_sensitive_content("url?pass=abc123&user=bob"),
            "url?pass=[REDACTED]&user=bob"
        );
    }

    #[test]
    fn test_redact_multiple_password_occurrences() {
        assert_eq!(
            redact_sensitive_content("password=first&password=second"),
            "password=[REDACTED]&password=[REDACTED]"
        );
    }
}
