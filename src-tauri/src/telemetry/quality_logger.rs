//! Synchronized JSONL quality event writer/reader.

use serde_json::Value;
use std::fs::{create_dir_all, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static QUALITY_LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

fn quality_lock() -> &'static Mutex<()> {
    QUALITY_LOG_LOCK.get_or_init(|| Mutex::new(()))
}

pub fn quality_dir(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("quality")
}

pub fn append_quality_event(
    app_data_dir: &Path,
    file_name: &str,
    event: &Value,
) -> Result<(), String> {
    let _guard = quality_lock()
        .lock()
        .map_err(|_| "quality log mutex poisoned".to_string())?;

    let dir = quality_dir(app_data_dir);
    create_dir_all(&dir).map_err(|err| err.to_string())?;
    let path = dir.join(file_name);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|err| err.to_string())?;
    let line = serde_json::to_string(event).map_err(|err| err.to_string())?;
    file.write_all(line.as_bytes())
        .map_err(|err| err.to_string())?;
    file.write_all(b"\n").map_err(|err| err.to_string())?;
    file.flush().map_err(|err| err.to_string())?;
    Ok(())
}

pub fn read_quality_events(
    app_data_dir: &Path,
    file_name: &str,
    limit: usize,
) -> Result<(Vec<Value>, usize), String> {
    let path = quality_dir(app_data_dir).join(file_name);
    if !path.exists() {
        return Ok((Vec::new(), 0));
    }
    let file = File::open(path).map_err(|err| err.to_string())?;
    let reader = BufReader::new(file);
    let mut rows = Vec::new();
    let mut malformed = 0usize;
    for line in reader.lines() {
        let line = match line {
            Ok(value) => value,
            Err(_) => {
                malformed += 1;
                continue;
            }
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        match serde_json::from_str::<Value>(trimmed) {
            Ok(value) => rows.push(value),
            Err(_) => malformed += 1,
        }
    }
    if limit > 0 && rows.len() > limit {
        let start = rows.len() - limit;
        rows = rows[start..].to_vec();
    }
    Ok((rows, malformed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    fn temp_quality_dir(label: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!(
            "continuum-quality-logger-{label}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time")
                .as_nanos()
        ));
        create_dir_all(&base).expect("create temp quality dir");
        base
    }

    #[test]
    fn concurrent_appends_produce_parseable_jsonl_rows() {
        let app_dir = Arc::new(temp_quality_dir("concurrent"));
        let file_name = "signals.jsonl";
        let thread_count = 8usize;
        let rows_per_thread = 40usize;
        let mut handles = Vec::new();

        for thread_index in 0..thread_count {
            let app_dir = Arc::clone(&app_dir);
            handles.push(std::thread::spawn(move || {
                for row_index in 0..rows_per_thread {
                    append_quality_event(
                        app_dir.as_path(),
                        file_name,
                        &json!({
                            "payload": {
                                "timestamp_ms": 1_700_000_000_000i64 + (thread_index * 1_000 + row_index) as i64,
                                "thread": thread_index,
                                "row": row_index
                            }
                        }),
                    )
                    .expect("append quality row");
                }
            }));
        }

        for handle in handles {
            handle.join().expect("join quality write thread");
        }

        let (rows, malformed) =
            read_quality_events(app_dir.as_path(), file_name, 0).expect("read quality rows");
        assert_eq!(malformed, 0);
        assert_eq!(rows.len(), thread_count * rows_per_thread);
        assert!(rows.iter().all(|row| row.get("payload").is_some()));
    }
}
