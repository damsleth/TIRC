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
}

impl LogWriter {
    pub fn new(root: impl Into<PathBuf>, retention_days: u16) -> Self {
        Self {
            root: root.into(),
            retention_days,
        }
    }

    pub async fn append(&self, record: &LogRecord) -> Result<PathBuf, LogError> {
        let date = record.ts.date_naive();
        let file = self
            .root
            .join(sanitize(&record.network))
            .join(sanitize(&record.buffer))
            .join(format!(
                "{:04}-{:02}-{:02}.jsonl",
                date.year(),
                date.month(),
                date.day()
            ));

        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).await?;
        }

        let line = serde_json::to_string(record)?;
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
