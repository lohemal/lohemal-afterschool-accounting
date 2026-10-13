//! 백업 — SQLite Online Backup API 로 일관된 스냅샷을 뜨고, 보관 중인 원본 엑셀과 함께 하나의 zip 패키지로 묶는다.
//!
//!   · 실행 중인 DB 파일을 복사하지 않는다(WAL 에만 있는 내용을 놓친다). `rusqlite::backup::Backup` 이 스냅샷을 만든다.
//!   · 패키지 = manifest.json + asaccounting.db + uploads/<sha256>.<ext>…  파일마다 SHA-256, 전체 package_sha256.
//!   · 만든 뒤 반드시 검증(구조·체크섬·integrity_check·외래키·주요 표·메타데이터)하고 결과를 곁 파일(.verify.json)에 남긴다.
//!     검증을 통과한 것만 정상 목록에 보인다.
//!   · 자동 백업은 최근 N개만 남긴다. 수동·마이그레이션·복원 전 백업은 지우지 않는다. 정리 내역은 작업 이력에 남는다.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::migrate;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::ops::history;

pub const FORMAT_VERSION: u32 = 1;
pub const DB_IN_PACKAGE: &str = "asaccounting.db";
pub const DEFAULT_KEEP: i64 = 10;

pub fn kind_label(k: &str) -> &'static str {
    match k {
        "manual" => "수동",
        "auto" => "자동(정기)",
        "migration" => "마이그레이션 직전",
        "pre_restore" => "복원 직전 안전 백업",
        _ => "알 수 없음",
    }
}

pub fn is_protected(kind: &str) -> bool {
    matches!(kind, "manual" | "migration" | "pre_restore")
}

pub fn backups_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

fn sha256_file(path: &Path) -> AppResult<(String, u64)> {
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    let mut size = 0u64;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        h.update(&buf[..n]);
    }
    Ok((hex::encode(h.finalize()), size))
}

fn package_hash(files: &[ManifestFile]) -> String {
    let mut h = Sha256::new();
    for f in files {
        h.update(format!("{}:{}\n", f.path, f.sha256).as_bytes());
    }
    hex::encode(h.finalize())
}

fn io_err(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::new("BACKUP", format!("{what}")).detail(e.to_string())
}

/// 주요 표 건수 — 백업 메타데이터와 복원 미리보기에 쓴다.
pub fn counts(conn: &Connection) -> AppResult<DbCounts> {
    let has = |t: &str| -> AppResult<bool> { Ok(conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [t], |r| r.get::<_, i64>(0))? > 0) };
    let cnt = |sql: &str| -> AppResult<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let opt = |t: &str, sql: &str| -> AppResult<i64> { if has(t)? { cnt(sql) } else { Ok(0) } };
    let mut years = Vec::new();
    if has("school_year")? {
        let mut st = conn.prepare("SELECT name FROM school_year ORDER BY year")?;
        years = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    }
    Ok(DbCounts {
        schema_version: migrate::current_version(conn)?,
        years,
        ledger_entries: opt("ledger_entry", "SELECT count(*) FROM ledger_entry")?,
        active_entries: opt("ledger_entry", "SELECT count(*) FROM ledger_entry WHERE state = 'active'")?,
        adjustments: opt("adjustment", "SELECT count(*) FROM adjustment")?,
        refunds: opt("adjustment", "SELECT count(*) FROM adjustment WHERE kind IN ('refund', 'refund_return')")?,
        voucher_targets: opt("voucher_target", "SELECT count(*) FROM voucher_target")?,
        voucher_batches: opt("voucher_batch", "SELECT count(*) FROM voucher_batch")?,
        uploads: opt("upload_file", "SELECT count(*) FROM upload_file")?,
        students: opt("student", "SELECT count(*) FROM student")?,
        audit_logs: opt("audit_log", "SELECT count(*) FROM audit_log")?,
        month_closes: opt("month_close", "SELECT count(*) FROM month_close")?,
    })
}

fn stored_names(conn: &Connection) -> AppResult<Vec<String>> {
    let has: i64 = conn.query_row("SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'upload_file'", [], |r| r.get(0))?;
    if has == 0 {
        return Ok(vec![]);
    }
    let cols: Vec<String> = {
        let mut st = conn.prepare("PRAGMA table_info(upload_file)")?;
        let v: Vec<String> = st.query_map([], |r| r.get::<_, String>(1))?.collect::<Result<_, _>>()?;
        v
    };
    if !cols.iter().any(|c| c == "stored_name") {
        return Ok(vec![]);
    }
    let mut st = conn.prepare("SELECT DISTINCT stored_name FROM upload_file WHERE stored_name <> '' ORDER BY stored_name")?;
    let v = st.query_map([], |r| r.get::<_, String>(0))?.collect::<Result<_, _>>()?;
    Ok(v)
}

/// 백업 패키지 생성 + 검증. 검증 결과는 곁 파일에 남고 작업 이력에 기록된다.
pub fn create(conn: &Connection, data_dir: &Path, kind: &str, note: &str) -> AppResult<BackupInfo> {
    if !matches!(kind, "manual" | "auto" | "migration" | "pre_restore") {
        return Err(AppError::invalid("백업 종류가 올바르지 않습니다."));
    }
    let dir = backups_dir(data_dir);
    std::fs::create_dir_all(&dir)?;
    let stamp = chrono::Local::now();
    let mut file_name = format!("방과후회계백업_{}_{}.zip", stamp.format("%Y%m%d_%H%M%S"), kind_label(kind).split('(').next().unwrap_or("").trim());
    let mut n = 1;
    while dir.join(&file_name).exists() {
        n += 1;
        file_name = format!("방과후회계백업_{}_{}_{}.zip", stamp.format("%Y%m%d_%H%M%S"), kind_label(kind).split('(').next().unwrap_or("").trim(), n);
    }
    let zip_path = dir.join(&file_name);
    let tmp = tempfile::Builder::new().prefix(".backup-").tempdir_in(&dir).map_err(|e| io_err("임시 폴더를 만들지 못했습니다.", e))?;
    // 1. 일관된 DB 스냅샷 (Online Backup API)
    let snap = tmp.path().join(DB_IN_PACKAGE);
    {
        let mut dst = Connection::open(&snap)?;
        {
            let bk = rusqlite::backup::Backup::new(conn, &mut dst)?;
            bk.run_to_completion(256, std::time::Duration::from_millis(5), None)?;
        }
        dst.pragma_update(None, "journal_mode", "DELETE")?;
        dst.close().map_err(|(_, e)| io_err("스냅샷을 닫지 못했습니다.", e))?;
    }
    let counts = {
        let c = Connection::open_with_flags(&snap, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        counts(&c)?
    };
    // 2. 원본 파일
    let uploads = crate::repo::upload::uploads_dir(data_dir);
    let mut files: Vec<ManifestFile> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    let (db_sha, db_size) = sha256_file(&snap)?;
    files.push(ManifestFile { path: DB_IN_PACKAGE.into(), sha256: db_sha, size: db_size });
    let mut sources: Vec<(String, PathBuf)> = vec![(DB_IN_PACKAGE.into(), snap.clone())];
    for name in stored_names(conn)? {
        let p = uploads.join(&name);
        if p.is_file() {
            let (sha, size) = sha256_file(&p)?;
            files.push(ManifestFile { path: format!("uploads/{name}"), sha256: sha, size });
            sources.push((format!("uploads/{name}"), p));
        } else {
            missing.push(name);
        }
    }
    let manifest = BackupManifest {
        format_version: FORMAT_VERSION,
        created_at: stamp.format("%Y-%m-%d %H:%M:%S").to_string(),
        app_version: env!("CARGO_PKG_VERSION").into(),
        schema_version: counts.schema_version,
        kind: kind.into(),
        note: note.trim().into(),
        years: counts.years.clone(),
        db_file: DB_IN_PACKAGE.into(),
        package_sha256: package_hash(&files),
        files,
        complete: missing.is_empty(),
        missing_files: missing,
        counts,
    };
    // 3. zip
    {
        let f = std::fs::File::create(&zip_path)?;
        let mut z = zip::ZipWriter::new(f);
        let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
        z.start_file("manifest.json", opt).map_err(|e| io_err("패키지를 쓰지 못했습니다.", e))?;
        z.write_all(serde_json::to_string_pretty(&manifest).unwrap_or_default().as_bytes())?;
        for (name, src) in &sources {
            z.start_file(name, opt).map_err(|e| io_err("패키지를 쓰지 못했습니다.", e))?;
            let mut r = std::fs::File::open(src)?;
            std::io::copy(&mut r, &mut z)?;
        }
        z.finish().map_err(|e| io_err("패키지를 닫지 못했습니다.", e))?;
    }
    drop(tmp);
    // 4. 검증
    let v = verify(&zip_path)?;
    write_sidecar(&zip_path, &v)?;
    let info = info_of(&zip_path)?;
    history::record(
        data_dir,
        "backup",
        if v.ok { "ok" } else { "failed" },
        &format!("{} 백업 {} ({}, {} 파일, {})", kind_label(kind), if v.ok { "생성·검증 완료" } else { "검증 실패" }, file_name, manifest.files.len(), if manifest.complete { "원본 포함" } else { "원본 일부 없음" }),
        Some(&file_name),
        serde_json::json!({ "errors": v.errors, "warnings": v.warnings, "schemaVersion": manifest.schema_version }),
    )?;
    if !v.ok {
        return Err(AppError::new("BACKUP_VERIFY", format!("백업 파일을 만들었지만 검증에 실패했습니다: {}", v.errors.join(" / "))));
    }
    Ok(info)
}

fn sidecar_path(zip_path: &Path) -> PathBuf {
    PathBuf::from(format!("{}.verify.json", zip_path.display()))
}

fn write_sidecar(zip_path: &Path, v: &BackupVerify) -> AppResult<()> {
    let s = serde_json::json!({ "ok": v.ok, "checkedAt": v.checked_at, "errors": v.errors, "warnings": v.warnings });
    std::fs::write(sidecar_path(zip_path), s.to_string())?;
    Ok(())
}

fn read_sidecar(zip_path: &Path) -> Option<(bool, Vec<String>)> {
    let text = std::fs::read_to_string(sidecar_path(zip_path)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((v["ok"].as_bool().unwrap_or(false), v["errors"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default()))
}

/// manifest 만 읽는다 (목록용).
pub fn read_manifest(zip_path: &Path) -> AppResult<BackupManifest> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| io_err("백업 패키지를 열지 못했습니다.", e))?;
    let mut m = z.by_name("manifest.json").map_err(|e| io_err("백업 패키지에 manifest.json 이 없습니다.", e))?;
    let mut text = String::new();
    m.read_to_string(&mut text)?;
    serde_json::from_str(&text).map_err(|e| io_err("manifest.json 을 읽지 못했습니다.", e))
}

/// 패키지 검증 — 구조 · 체크섬 · 전체 해시 · SQLite integrity_check · 외래키 · 주요 표 · 메타데이터 일치.
pub fn verify(zip_path: &Path) -> AppResult<BackupVerify> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let checked_at = history::now();
    if !zip_path.is_file() {
        return Ok(BackupVerify { ok: false, checked_at, errors: vec!["백업 파일이 없습니다.".into()], warnings });
    }
    let manifest = match read_manifest(zip_path) {
        Ok(m) => m,
        Err(e) => return Ok(BackupVerify { ok: false, checked_at, errors: vec![format!("{} ({})", e.message, e.detail.unwrap_or_default())], warnings }),
    };
    if manifest.format_version > FORMAT_VERSION {
        errors.push(format!("지원하지 않는 백업 형식(v{})입니다. 프로그램을 업데이트해 주세요.", manifest.format_version));
    }
    if manifest.package_sha256 != package_hash(&manifest.files) {
        errors.push("백업 전체 무결성 값(package_sha256)이 맞지 않습니다.".into());
    }
    if !manifest.files.iter().any(|f| f.path == DB_IN_PACKAGE) {
        errors.push("백업 패키지에 DB 파일 항목이 없습니다.".into());
    }
    let f = std::fs::File::open(zip_path)?;
    let mut z = match zip::ZipArchive::new(f) {
        Ok(z) => z,
        Err(e) => return Ok(BackupVerify { ok: false, checked_at, errors: vec![format!("백업 패키지 구조가 깨졌습니다: {e}")], warnings }),
    };
    let tmp = tempfile::tempdir().map_err(|e| io_err("임시 폴더를 만들지 못했습니다.", e))?;
    let mut db_tmp: Option<PathBuf> = None;
    for mf in &manifest.files {
        let mut entry = match z.by_name(&mf.path) {
            Ok(e) => e,
            Err(_) => {
                errors.push(format!("패키지에 '{}' 이(가) 없습니다.", mf.path));
                continue;
            }
        };
        let mut h = Sha256::new();
        let mut buf = [0u8; 65536];
        let mut size = 0u64;
        let mut out: Option<std::fs::File> = if mf.path == DB_IN_PACKAGE {
            let p = tmp.path().join("check.db");
            db_tmp = Some(p.clone());
            Some(std::fs::File::create(&p)?)
        } else {
            None
        };
        loop {
            let n = match entry.read(&mut buf) {
                Ok(n) => n,
                Err(e) => {
                    errors.push(format!("'{}' 을(를) 읽는 중 오류: {e}", mf.path));
                    break;
                }
            };
            if n == 0 {
                break;
            }
            size += n as u64;
            h.update(&buf[..n]);
            if let Some(o) = out.as_mut() {
                o.write_all(&buf[..n])?;
            }
        }
        let sha = hex::encode(h.finalize());
        if sha != mf.sha256 || size != mf.size {
            errors.push(format!("'{}' 체크섬이 맞지 않습니다 (손상 또는 변조).", mf.path));
            if mf.path == DB_IN_PACKAGE {
                db_tmp = None;
            }
        }
    }
    if let Some(p) = db_tmp {
        match Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY) {
            Ok(c) => {
                let ic: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0)).unwrap_or_else(|e| format!("실패: {e}"));
                if ic != "ok" {
                    errors.push(format!("SQLite 무결성 검사 실패: {ic}"));
                }
                let fk = c.prepare("PRAGMA foreign_key_check").and_then(|mut st| st.query_map([], |_| Ok(())).map(|r| r.count()));
                match fk {
                    Ok(0) => {}
                    Ok(n) => errors.push(format!("외래 키 위반 {n}건")),
                    Err(e) => errors.push(format!("외래 키 검사 실패: {e}")),
                }
                for t in ["school_year", "fiscal_month", "ledger_entry", "upload_file", "audit_log"] {
                    if c.query_row(&format!("SELECT count(*) FROM {t}"), [], |r| r.get::<_, i64>(0)).is_err() {
                        errors.push(format!("주요 표 '{t}' 에 접근할 수 없습니다."));
                    }
                }
                match migrate::current_version(&c) {
                    Ok(v) if v == manifest.schema_version => {}
                    Ok(v) => errors.push(format!("메타데이터의 스키마 버전(v{})과 DB(v{v})가 다릅니다.", manifest.schema_version)),
                    Err(e) => errors.push(format!("스키마 버전을 읽지 못했습니다: {e}")),
                }
                if let Ok(cn) = counts(&c) {
                    if cn.ledger_entries != manifest.counts.ledger_entries || cn.audit_logs != manifest.counts.audit_logs {
                        errors.push("메타데이터의 건수와 DB 건수가 다릅니다.".into());
                    }
                }
            }
            Err(e) => errors.push(format!("백업 DB 를 열지 못했습니다: {e}")),
        }
    } else if errors.is_empty() {
        errors.push("백업 DB 파일을 꺼내지 못했습니다.".into());
    }
    if !manifest.complete {
        warnings.push(format!("원본 파일 {}개가 백업 당시 보관 폴더에 없어 패키지에 들어 있지 않습니다 (완전한 백업이 아님).", manifest.missing_files.len()));
    }
    Ok(BackupVerify { ok: errors.is_empty(), checked_at, errors, warnings })
}

pub fn info_of(zip_path: &Path) -> AppResult<BackupInfo> {
    let size = std::fs::metadata(zip_path)?.len();
    let file_name = zip_path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    match read_manifest(zip_path) {
        Ok(m) => {
            let (verified, errs) = read_sidecar(zip_path).unwrap_or((false, vec!["검증 기록이 없습니다.".into()]));
            Ok(BackupInfo {
                file_name,
                path: zip_path.display().to_string(),
                size,
                kind_label: kind_label(&m.kind).into(),
                protected: is_protected(&m.kind),
                kind: m.kind,
                created_at: m.created_at,
                app_version: m.app_version,
                schema_version: m.schema_version,
                years: m.years,
                complete: m.complete,
                file_count: m.files.len() as i64,
                verified,
                verify_errors: errs,
                counts: Some(m.counts),
                legacy: false,
            })
        }
        Err(e) => Ok(BackupInfo {
            file_name,
            path: zip_path.display().to_string(),
            size,
            kind: "unknown".into(),
            kind_label: "알 수 없음".into(),
            created_at: String::new(),
            app_version: String::new(),
            schema_version: 0,
            years: vec![],
            complete: false,
            file_count: 0,
            verified: false,
            verify_errors: vec![e.message],
            counts: None,
            protected: true,
            legacy: false,
        }),
    }
}

/// 백업 목록 — 검증을 통과한 것과 실패·미검증·이전 형식을 나눈다. 최신 순.
pub fn list(conn: &Connection, data_dir: &Path) -> AppResult<BackupList> {
    let dir = backups_dir(data_dir);
    let mut backups = Vec::new();
    let mut failed = Vec::new();
    if dir.is_dir() {
        for e in std::fs::read_dir(&dir)? {
            let p = e?.path();
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if name.ends_with(".zip") {
                let info = info_of(&p)?;
                if info.verified {
                    backups.push(info);
                } else {
                    failed.push(info);
                }
            } else if name.ends_with(".db") {
                let meta = std::fs::metadata(&p)?;
                let created = meta.modified().ok().map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
                failed.push(BackupInfo {
                    file_name: name,
                    path: p.display().to_string(),
                    size: meta.len(),
                    kind: "legacy".into(),
                    kind_label: "이전 형식(.db)".into(),
                    created_at: created,
                    app_version: String::new(),
                    schema_version: 0,
                    years: vec![],
                    complete: false,
                    file_count: 1,
                    verified: false,
                    verify_errors: vec!["이전 형식의 DB 단일 파일 백업입니다. 이 화면에서는 복원할 수 없습니다.".into()],
                    counts: None,
                    protected: true,
                    legacy: true,
                });
            }
        }
    }
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    failed.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let auto_enabled = crate::repo::setting::get(conn, "backup.auto")?.map(|v| v != "0").unwrap_or(true);
    let keep = crate::repo::setting::get(conn, "backup.keep")?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(DEFAULT_KEEP).max(1);
    Ok(BackupList { dir: dir.display().to_string(), last_backup_at: backups.first().map(|b| b.created_at.clone()), backups, failed, auto_enabled, keep })
}

/// 자동 백업 정리 — 검증된 자동 백업 중 최근 `keep` 개만 남긴다. 보호 종류는 건드리지 않는다. 삭제 내역은 이력에 남긴다.
pub fn cleanup(data_dir: &Path, keep: i64) -> AppResult<Vec<String>> {
    let dir = backups_dir(data_dir);
    let mut autos: Vec<BackupInfo> = Vec::new();
    if dir.is_dir() {
        for e in std::fs::read_dir(&dir)? {
            let p = e?.path();
            if p.extension().map(|x| x == "zip").unwrap_or(false) {
                let info = info_of(&p)?;
                if info.kind == "auto" {
                    autos.push(info);
                }
            }
        }
    }
    autos.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let mut removed = Vec::new();
    for b in autos.into_iter().skip(keep.max(1) as usize) {
        let p = PathBuf::from(&b.path);
        std::fs::remove_file(&p)?;
        let _ = std::fs::remove_file(sidecar_path(&p));
        history::record(data_dir, "cleanup", "ok", &format!("자동 백업 정리: {} 삭제 (보관 {}개 초과)", b.file_name, keep), Some(&b.file_name), serde_json::json!({ "reason": "retention", "keep": keep }))?;
        removed.push(b.file_name);
    }
    Ok(removed)
}

/// 정기(하루 한 번) 자동 백업 — 프로그램 시작 때 부른다. 오늘 자동 백업이 있으면 건너뛴다.
pub fn auto_daily(conn: &Connection, data_dir: &Path) -> AppResult<Option<BackupInfo>> {
    let enabled = crate::repo::setting::get(conn, "backup.auto")?.map(|v| v != "0").unwrap_or(true);
    if !enabled {
        return Ok(None);
    }
    let keep = crate::repo::setting::get(conn, "backup.keep")?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(DEFAULT_KEEP).max(1);
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let l = list(conn, data_dir)?;
    if l.backups.iter().any(|b| b.kind == "auto" && b.created_at.starts_with(&today)) {
        return Ok(None);
    }
    let info = create(conn, data_dir, "auto", "정기 자동 백업")?;
    cleanup(data_dir, keep)?;
    Ok(Some(info))
}
