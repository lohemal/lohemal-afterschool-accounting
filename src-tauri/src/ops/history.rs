//! 작업 이력 — 자료 폴더의 `ops-history.jsonl` (한 줄에 한 작업, 덧붙이기만). DB 와 독립이라 복원해도 남는다.

use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::AppResult;
use crate::model::OpsRecord;

pub fn file_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ops-history.jsonl")
}

pub fn now() -> String {
    chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

pub fn record(data_dir: &Path, kind: &str, status: &str, summary: &str, file: Option<&str>, detail: serde_json::Value) -> AppResult<OpsRecord> {
    let rec = OpsRecord { at: now(), kind: kind.into(), status: status.into(), summary: summary.into(), file: file.map(|s| s.to_string()), detail };
    std::fs::create_dir_all(data_dir)?;
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(file_path(data_dir))?;
    let line = serde_json::to_string(&rec).map_err(|e| crate::error::AppError::new("IO", "작업 이력을 쓰지 못했습니다.").detail(e.to_string()))?;
    writeln!(f, "{line}")?;
    Ok(rec)
}

/// 최신 순.
pub fn list(data_dir: &Path, limit: usize) -> AppResult<Vec<OpsRecord>> {
    let p = file_path(data_dir);
    if !p.exists() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&p)?;
    let mut out: Vec<OpsRecord> = text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect();
    out.reverse();
    out.truncate(limit);
    Ok(out)
}
