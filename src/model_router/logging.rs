use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::config::{RouterLoggingConfig, expand_tilde};
use crate::error::Result;
use crate::model_router::RequestLog;

#[derive(Clone, Default)]
pub struct RouterLogSink {
    memory_tx: Option<broadcast::Sender<RequestLog>>,
    file_writer: Option<Arc<Mutex<FileLogWriter>>>,
}

impl RouterLogSink {
    pub fn new(
        memory_tx: Option<broadcast::Sender<RequestLog>>,
        file_writer: Option<FileLogWriter>,
    ) -> Self {
        Self {
            memory_tx,
            file_writer: file_writer.map(|writer| Arc::new(Mutex::new(writer))),
        }
    }

    pub fn emit(&self, log: RequestLog) {
        if let Some(memory_tx) = &self.memory_tx {
            let _ = memory_tx.send(log.clone());
        }

        if let Some(file_writer) = &self.file_writer {
            match file_writer.lock() {
                Ok(mut guard) => {
                    if let Err(err) = guard.write(&log) {
                        tracing::warn!("router access log write failed: {err}");
                    }
                }
                Err(err) => {
                    tracing::warn!("router access log lock failed: {err}");
                }
            }
        }
    }
}

pub fn build_file_writer(config: &RouterLoggingConfig) -> Result<Option<FileLogWriter>> {
    if !config.enabled {
        return Ok(None);
    }

    let file_path = resolve_log_path(&config.file_path);
    let writer = FileLogWriter::new(
        file_path,
        config.max_file_size_bytes(),
        config.max_files_or_default(),
    )?;
    Ok(Some(writer))
}

pub fn resolve_log_path(file_path: &str) -> PathBuf {
    expand_tilde(Path::new(file_path))
}

pub struct FileLogWriter {
    base_path: PathBuf,
    max_file_size_bytes: u64,
    max_files: u32,
}

impl FileLogWriter {
    pub fn new(base_path: PathBuf, max_file_size_bytes: u64, max_files: u32) -> io::Result<Self> {
        if let Some(parent) = base_path.parent() {
            fs::create_dir_all(parent)?;
        }

        Ok(Self {
            base_path,
            max_file_size_bytes: max_file_size_bytes.max(1),
            max_files: max_files.max(1),
        })
    }

    pub fn write(&mut self, log: &RequestLog) -> io::Result<()> {
        let serialized = serde_json::to_string(log).map_err(io::Error::other)?;
        let incoming_len = serialized.len() as u64 + 1;
        self.rotate_if_needed(incoming_len)?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.base_path)?;
        file.write_all(serialized.as_bytes())?;
        file.write_all(b"\n")?;
        file.flush()?;

        Ok(())
    }

    fn rotate_if_needed(&self, incoming_len: u64) -> io::Result<()> {
        let current_len = match fs::metadata(&self.base_path) {
            Ok(meta) => meta.len(),
            Err(err) if err.kind() == io::ErrorKind::NotFound => 0,
            Err(err) => return Err(err),
        };

        if current_len + incoming_len <= self.max_file_size_bytes {
            return Ok(());
        }

        let oldest = rotated_path(&self.base_path, self.max_files);
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }

        for idx in (1..self.max_files).rev() {
            let src = rotated_path(&self.base_path, idx);
            let dst = rotated_path(&self.base_path, idx + 1);
            if src.exists() {
                fs::rename(src, dst)?;
            }
        }

        if self.base_path.exists() {
            fs::rename(&self.base_path, rotated_path(&self.base_path, 1))?;
        }

        Ok(())
    }
}

fn rotated_path(base_path: &Path, idx: u32) -> PathBuf {
    PathBuf::from(format!("{}.{}", base_path.display(), idx))
}

pub struct FileLogTailer {
    base_path: PathBuf,
    offset: u64,
    pending_fragment: String,
    total_parse_errors: u64,
    max_files: u32,
}

pub struct TailPoll {
    pub logs: Vec<RequestLog>,
    pub waiting_for_file: bool,
}

impl FileLogTailer {
    pub fn new(base_path: PathBuf, max_files: u32) -> Self {
        Self {
            base_path,
            offset: 0,
            pending_fragment: String::new(),
            total_parse_errors: 0,
            max_files: max_files.max(1),
        }
    }

    pub fn load_recent(&mut self, max_entries: usize) -> io::Result<Vec<RequestLog>> {
        let mut logs = VecDeque::with_capacity(max_entries);

        for path in self.rotated_history_paths() {
            if !path.exists() {
                continue;
            }

            let mut pending = String::new();
            let (items, _) =
                read_logs_from_path(&path, 0, &mut pending, &mut self.total_parse_errors)?;
            for log in items {
                if logs.len() == max_entries {
                    logs.pop_front();
                }
                logs.push_back(log);
            }
        }

        if !self.base_path.exists() {
            self.offset = 0;
            self.pending_fragment.clear();
            return Ok(logs.into_iter().collect());
        }

        self.offset = 0;
        self.pending_fragment.clear();
        let (items, bytes_read) = read_logs_from_path(
            &self.base_path,
            self.offset,
            &mut self.pending_fragment,
            &mut self.total_parse_errors,
        )?;
        self.offset += bytes_read;
        for log in items {
            if logs.len() == max_entries {
                logs.pop_front();
            }
            logs.push_back(log);
        }

        Ok(logs.into_iter().collect())
    }

    pub fn poll(&mut self) -> io::Result<TailPoll> {
        if !self.base_path.exists() {
            self.offset = 0;
            self.pending_fragment.clear();
            return Ok(TailPoll {
                logs: Vec::new(),
                waiting_for_file: true,
            });
        }

        let len = fs::metadata(&self.base_path)?.len();
        if len < self.offset {
            self.offset = 0;
            self.pending_fragment.clear();
        }

        if len == self.offset {
            return Ok(TailPoll {
                logs: Vec::new(),
                waiting_for_file: false,
            });
        }

        let (logs, bytes_read) = read_logs_from_path(
            &self.base_path,
            self.offset,
            &mut self.pending_fragment,
            &mut self.total_parse_errors,
        )?;

        self.offset += bytes_read;

        Ok(TailPoll {
            logs,
            waiting_for_file: false,
        })
    }

    pub fn total_parse_errors(&self) -> u64 {
        self.total_parse_errors
    }

    fn rotated_history_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::with_capacity(self.max_files as usize);
        for idx in (1..=self.max_files).rev() {
            paths.push(rotated_path(&self.base_path, idx));
        }
        paths
    }
}

fn read_logs_from_path(
    path: &Path,
    start_offset: u64,
    pending_fragment: &mut String,
    total_parse_errors: &mut u64,
) -> io::Result<(Vec<RequestLog>, u64)> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(start_offset))?;
    let mut reader = BufReader::new(file);
    let mut logs = Vec::new();
    let mut bytes_read = 0_u64;

    loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line)?;
        if bytes == 0 {
            break;
        }
        bytes_read += bytes as u64;

        if !line.ends_with('\n') {
            pending_fragment.push_str(&line);
            break;
        }

        let mut full = String::new();
        if !pending_fragment.is_empty() {
            full.push_str(pending_fragment);
            pending_fragment.clear();
        }
        full.push_str(line.trim_end_matches(['\r', '\n']));

        if full.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<RequestLog>(&full) {
            Ok(log) => logs.push(log),
            Err(_) => *total_parse_errors += 1,
        }
    }

    Ok((logs, bytes_read))
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{FileLogTailer, FileLogWriter, rotated_path};
    use crate::model_router::RequestLog;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("vibemate-{name}-{unique}.log"))
    }

    fn sample_log(path: &str, status: u16) -> RequestLog {
        RequestLog {
            timestamp: RequestLog::now_timestamp(),
            request_id: "req-1".to_string(),
            method: "POST".to_string(),
            path: path.to_string(),
            original_model: "gpt-4o".to_string(),
            routed_model: "gpt-4o-mini".to_string(),
            provider: "openai".to_string(),
            status,
            latency_ms: 23,
            stream: false,
            error_summary: String::new(),
        }
    }

    #[test]
    fn request_log_json_roundtrip_preserves_fields() {
        let log = RequestLog {
            timestamp: "2026-03-01T12:34:56.789+08:00".to_string(),
            request_id: "req-100".to_string(),
            method: "POST".to_string(),
            path: "/v1/chat/completions".to_string(),
            original_model: "gpt-4o".to_string(),
            routed_model: "gpt-4o-mini".to_string(),
            provider: "openrouter".to_string(),
            status: 200,
            latency_ms: 42,
            stream: true,
            error_summary: String::new(),
        };
        let encoded = serde_json::to_string(&log).expect("serialize should succeed");
        let decoded: RequestLog = serde_json::from_str(&encoded).expect("parse should succeed");
        assert_eq!(decoded, log);
    }

    #[test]
    fn file_log_writer_rotates_when_size_exceeded() {
        let base_path = temp_path("rotate");
        let mut writer =
            FileLogWriter::new(base_path.clone(), 200, 3).expect("writer create should succeed");

        for i in 0..20 {
            let mut log = sample_log("/v1/responses", 200);
            log.request_id = format!("req-{i}");
            writer.write(&log).expect("write should succeed");
        }

        assert!(base_path.exists());
        assert!(rotated_path(&base_path, 1).exists());
        assert!(rotated_path(&base_path, 2).exists() || rotated_path(&base_path, 3).exists());

        let _ = std::fs::remove_file(&base_path);
        let _ = std::fs::remove_file(rotated_path(&base_path, 1));
        let _ = std::fs::remove_file(rotated_path(&base_path, 2));
        let _ = std::fs::remove_file(rotated_path(&base_path, 3));
    }

    #[test]
    fn file_log_tailer_reads_incremental_updates() {
        let base_path = temp_path("tail");
        let mut writer =
            FileLogWriter::new(base_path.clone(), 10_000, 3).expect("writer create should work");
        let mut tailer = FileLogTailer::new(base_path.clone(), 3);

        writer
            .write(&sample_log("/v1/chat/completions", 200))
            .expect("first write should succeed");
        let first = tailer.poll().expect("first poll should succeed");
        assert_eq!(first.logs.len(), 1);

        writer
            .write(&sample_log("/v1/messages", 500))
            .expect("second write should succeed");
        let second = tailer.poll().expect("second poll should succeed");
        assert_eq!(second.logs.len(), 1);
        assert_eq!(second.logs[0].path, "/v1/messages");

        let _ = std::fs::remove_file(&base_path);
    }

    #[test]
    fn file_log_tailer_load_recent_tracks_partial_line_for_followup_poll() {
        let base_path = temp_path("tail-load-recent-partial");
        let mut writer =
            FileLogWriter::new(base_path.clone(), 10_000, 3).expect("writer create should work");
        let mut tailer = FileLogTailer::new(base_path.clone(), 3);

        let log1 = sample_log("/v1/chat/completions", 200);
        writer.write(&log1).expect("first write should succeed");

        let mut log2 = sample_log("/v1/messages", 200);
        log2.request_id = "req-2".to_string();
        let serialized2 = serde_json::to_string(&log2).expect("serialize should succeed");
        let split = serialized2.len() / 2;
        let (first_half, second_half) = serialized2.split_at(split);

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&base_path)
            .expect("open should succeed");
        file.write_all(first_half.as_bytes())
            .expect("partial write should succeed");
        file.flush().expect("flush should succeed");

        let initial = tailer
            .load_recent(1_000)
            .expect("history load should succeed");
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].path, "/v1/chat/completions");

        let mut file = OpenOptions::new()
            .append(true)
            .open(&base_path)
            .expect("reopen should succeed");
        file.write_all(second_half.as_bytes())
            .expect("second half write should succeed");
        file.write_all(b"\n").expect("newline write should succeed");
        file.flush().expect("flush should succeed");

        let next = tailer
            .poll()
            .expect("poll after partial completion should succeed");
        assert_eq!(next.logs.len(), 1);
        assert_eq!(next.logs[0].path, "/v1/messages");

        let _ = std::fs::remove_file(&base_path);
    }

    #[test]
    fn file_log_tailer_recovers_after_file_truncate() {
        let base_path = temp_path("tail-truncate");
        let mut writer =
            FileLogWriter::new(base_path.clone(), 10_000, 3).expect("writer create should work");
        let mut tailer = FileLogTailer::new(base_path.clone(), 3);

        writer
            .write(&sample_log("/v1/chat/completions", 200))
            .expect("write should succeed");
        let first = tailer.poll().expect("first poll should succeed");
        assert_eq!(first.logs.len(), 1);

        std::fs::write(&base_path, "").expect("truncate should succeed");
        writer
            .write(&sample_log("/v1/responses", 200))
            .expect("write after truncate should succeed");

        let second = tailer.poll().expect("second poll should succeed");
        assert_eq!(second.logs.len(), 1);
        assert_eq!(second.logs[0].path, "/v1/responses");

        let _ = std::fs::remove_file(&base_path);
    }
}
