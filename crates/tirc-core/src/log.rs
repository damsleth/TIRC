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
/// Patterns redacted:
/// - `/msg nickserv identify <password>` -> `/msg nickserv identify [REDACTED]`
/// - `/ns identify <password>` -> `/ns identify [REDACTED]`
/// - `AUTHENTICATE <payload>` -> `AUTHENTICATE [REDACTED]`
/// - Various password patterns
fn redact_sensitive_content(body: &str) -> String {
    let lower = body.to_ascii_lowercase();
    
    // Check for NickServ/NS identify patterns
    if lower.contains("identify") || lower.contains("register") {
        // Pattern: /msg nickserv identify <password>
        // Pattern: /ns identify <password>
        // Pattern: identify <password>
        let patterns = [
            "nickserv identify ",
            "nickserv register ",
            "ns identify ",
            "ns register ",
            "/identify ",
            " identify ",
        ];
        
        for pattern in patterns {
            if let Some(pos) = lower.find(pattern) {
                let start = pos + pattern.len();
                // Find end of password (next space or end of string)
                let password_end = body[start..]
                    .find(|c: char| c.is_whitespace())
                    .map(|p| start + p)
                    .unwrap_or(body.len());
                
                if password_end > start {
                    let mut result = body[..start].to_owned();
                    result.push_str("[REDACTED]");
                    if password_end < body.len() {
                        result.push_str(&body[password_end..]);
                    }
                    return result;
                }
            }
        }
    }
    
    // SASL AUTHENTICATE payload
    if lower.starts_with("authenticate ") && body.len() > 13 {
        return "AUTHENTICATE [REDACTED]".to_owned();
    }
    
    // Check for password= or password: patterns  
    if lower.contains("password") {
        let patterns = ["password=", "password:", "password "];
        for pattern in patterns {
            if let Some(pos) = lower.find(pattern) {
                let start = pos + pattern.len();
                let password_end = body[start..]
                    .find(|c: char| c.is_whitespace() || c == '&' || c == ';')
                    .map(|p| start + p)
                    .unwrap_or(body.len());
                
                if password_end > start {
                    let mut result = body[..start].to_owned();
                    result.push_str("[REDACTED]");
                    if password_end < body.len() {
                        result.push_str(&body[password_end..]);
                    }
                    return result;
                }
            }
        }
    }
    
    body.to_owned()
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
    fn test_redact_ns_identify() {
        assert_eq!(
            redact_sensitive_content("/ns identify secretpass"),
            "/ns identify [REDACTED]"
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
    fn test_no_redaction_normal_message() {
        let msg = "Hello everyone, how are you today?";
        assert_eq!(redact_sensitive_content(msg), msg);
    }

    #[test]
    fn test_redact_password_equals() {
        assert_eq!(
            redact_sensitive_content("some config password=secret123 other"),
            "some config password=[REDACTED] other"
        );
    }
}
