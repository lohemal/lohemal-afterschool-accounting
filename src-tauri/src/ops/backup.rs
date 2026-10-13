//! 백업 — SQLite Online Backup API 로 일관된 스냅샷을 뜨고, 보관 중인 원본 엑셀과 함께 하나의 zip 패키지로 묶는다.
//!
//!   · 실행 중인 DB 파일을 복사하지 않는다(WAL 에만 있는 내용을 놓친다). `rusqlite::backup::Backup` 이 스냅샷을 만든다.
//!   · 패키지 = manifest.json + asaccounting.db + uploads/<sha256>.<ext>…  파일마다 SHA-256, 전체 package_sha256.
//!   · P6: 패키지를 암호화한다(`.asbk`, AES-256-GCM). 현재 계정용(DPAPI, 자동 백업 기본) 또는 비밀번호(다른 PC 이전용).
//!     암호화하지 않은 옛 `.zip` 도 읽을 수 있지만 "암호화되지 않았습니다" 로 표시한다. 자동 변환·삭제는 하지 않는다.
//!   · 임시 파일(`.tmp-…`)로 만들어 검증까지 끝낸 뒤 최종 이름으로 바꾼다. 검증 실패본은 정상 목록에 보이지 않는다.
//!   · 목록용 메타(`.meta.json`)에는 개인정보가 없는 건수·버전·종류만 둔다. 파일명에도 학교·학생 정보가 없다.
//!   · 백업 폴더는 기본(자료 폴더/backups) 또는 사용자 지정(다른 드라이브·USB·네트워크 공유). 지정 경로가 안 되면 실패를 기록하고
//!     설정에 따라 기본 경로로 대체 백업하되 실제 저장 위치를 그대로 표시한다.
//!   · 자동 백업은 최근 N개만 남긴다. 수동·마이그레이션·복원 전·업데이트 전 백업은 지우지 않는다. 정리 내역은 작업 이력에 남는다.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use crate::db::migrate;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::ops::{crypto, history};
use crate::repo::setting;

pub const FORMAT_VERSION: u32 = 1;
pub const DB_IN_PACKAGE: &str = "asaccounting.db";
pub const DEFAULT_KEEP: i64 = 10;

pub fn kind_label(k: &str) -> &'static str {
    match k {
        "manual" => "수동",
        "auto" => "자동(정기)",
        "migration" => "마이그레이션 직전",
        "pre_restore" => "복원 직전 안전 백업",
        "pre_update" => "업데이트 직전",
        _ => "알 수 없음",
    }
}

pub fn is_protected(kind: &str) -> bool {
    matches!(kind, "manual" | "migration" | "pre_restore" | "pre_update")
}

pub fn encryption_label(e: &str) -> &'static str {
    match e {
        "dpapi" => "현재 Windows 계정용 암호화",
        "password" => "비밀번호 암호화",
        _ => "암호화 안 됨",
    }
}

pub fn default_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("backups")
}

/// 설정된 사용자 지정 경로 (비어 있으면 None).
pub fn custom_dir(conn: &Connection) -> AppResult<Option<PathBuf>> {
    Ok(setting::get(conn, "backup.dir")?.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).map(PathBuf::from))
}

/// 실제 쓰는 백업 폴더 (지정 경로가 있으면 그것).
pub fn resolve_dir(conn: &Connection, data_dir: &Path) -> AppResult<(PathBuf, bool)> {
    Ok(match custom_dir(conn)? {
        Some(p) => (p, true),
        None => (default_dir(data_dir), false),
    })
}

pub struct Settings {
    pub auto_enabled: bool,
    pub keep: i64,
    /// none · dpapi
    pub encryption: String,
    pub fallback: bool,
}

pub fn settings(conn: &Connection) -> AppResult<Settings> {
    Ok(Settings {
        auto_enabled: setting::get(conn, "backup.auto")?.map(|v| v != "0").unwrap_or(true),
        keep: setting::get(conn, "backup.keep")?.and_then(|v| v.parse::<i64>().ok()).unwrap_or(DEFAULT_KEEP).max(1),
        encryption: setting::get(conn, "backup.encrypt")?.filter(|v| v == "none" || v == "dpapi").unwrap_or_else(|| "dpapi".into()),
        fallback: setting::get(conn, "backup.fallback")?.map(|v| v != "0").unwrap_or(true),
    })
}

#[cfg(windows)]
fn free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        probe = probe.parent()?.to_path_buf();
    }
    let wide: Vec<u16> = probe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
    let mut free: u64 = 0;
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    if ok == 0 { None } else { Some(free) }
}

#[cfg(not(windows))]
fn free_space(_path: &Path) -> Option<u64> {
    None
}

/// 경로 점검 — 존재·쓰기 가능·남은 공간. `need_bytes` 만큼 남아 있어야 ok.
pub fn check_dir(path: &Path, need_bytes: u64) -> DirCheck {
    let exists = path.is_dir();
    let mut writable = false;
    let mut message = String::new();
    if !exists {
        if path.exists() {
            message = "폴더가 아니라 파일입니다.".into();
        } else {
            message = "폴더가 없거나 지금 접근할 수 없습니다 (USB·네트워크 연결을 확인하세요).".into();
        }
    } else {
        let probe = path.join(format!(".write-test-{}", std::process::id()));
        match std::fs::write(&probe, b"ok") {
            Ok(()) => {
                writable = true;
                let _ = std::fs::remove_file(&probe);
            }
            Err(e) => message = format!("이 폴더에 쓸 수 없습니다: {e}"),
        }
    }
    let free_bytes = if exists { free_space(path) } else { None };
    let mut ok = exists && writable;
    if ok {
        if let Some(f) = free_bytes {
            if f < need_bytes {
                ok = false;
                message = format!("저장 공간이 부족합니다 (남은 공간 {} MB, 필요 {} MB).", f / 1_048_576, need_bytes / 1_048_576);
            }
        }
    }
    DirCheck { path: path.display().to_string(), exists, writable, free_bytes, ok, message }
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
    AppError::new("BACKUP", what.to_string()).detail(e.to_string())
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

/// 생성 옵션.
pub struct CreateOpts<'a> {
    pub dir: PathBuf,
    /// none · dpapi · password
    pub encryption: &'a str,
    pub password: Option<&'a str>,
    /// 시험용 실패 지점: after_zip · after_encrypt
    pub fail_at: Option<&'a str>,
}

/// 설정대로 백업 (지정 경로·암호화 방식·대체 백업 반영). 수동 백업에 비밀번호를 주면 비밀번호 암호화.
pub fn create(conn: &Connection, data_dir: &Path, kind: &str, note: &str) -> AppResult<BackupInfo> {
    create_with_password(conn, data_dir, kind, note, None)
}

pub fn create_with_password(conn: &Connection, data_dir: &Path, kind: &str, note: &str, password: Option<&str>) -> AppResult<BackupInfo> {
    let s = settings(conn)?;
    let (dir, custom) = resolve_dir(conn, data_dir)?;
    let enc = if password.is_some() { "password" } else { s.encryption.as_str() };
    let first = create_opts(conn, data_dir, kind, note, &CreateOpts { dir: dir.clone(), encryption: enc, password, fail_at: None });
    match first {
        Ok(info) => Ok(info),
        Err(e) if custom && s.fallback => {
            // 지정 경로 실패 → 기록 → 기본 경로로 대체. 실제 저장 위치를 표시하고 지정 경로에 저장됐다고 하지 않는다.
            let fb = default_dir(data_dir);
            let mut info = create_opts(conn, data_dir, kind, &format!("{} (지정 경로 실패로 기본 경로에 대체 백업: {})", note, e.message), &CreateOpts { dir: fb, encryption: enc, password, fail_at: None })?;
            info.fallback = true;
            history::record(data_dir, "backup", "ok", &format!("지정 백업 경로({})를 쓸 수 없어 기본 경로에 대체 백업했습니다: {}", dir.display(), info.file_name), Some(&info.file_name), serde_json::json!({ "fallback": true, "customDir": dir.display().to_string(), "reason": e.message }))?;
            Ok(info)
        }
        Err(e) => Err(e),
    }
}

/// 백업 패키지 생성 + 검증. 임시 파일로 만들고 검증을 통과해야 최종 이름이 된다. 결과는 작업 이력에 남는다.
pub fn create_opts(conn: &Connection, data_dir: &Path, kind: &str, note: &str, opts: &CreateOpts) -> AppResult<BackupInfo> {
    if !matches!(kind, "manual" | "auto" | "migration" | "pre_restore" | "pre_update") {
        return Err(AppError::invalid("백업 종류가 올바르지 않습니다."));
    }
    let dir = &opts.dir;
    let result = create_inner(conn, data_dir, dir, kind, note, opts);
    match &result {
        Ok(info) => {
            history::record(
                data_dir,
                "backup",
                "ok",
                &format!("{} 백업 생성·검증 완료 ({}, {} 파일, {}, {}) → {}", kind_label(kind), info.file_name, info.file_count, if info.complete { "원본 포함" } else { "원본 일부 없음" }, encryption_label(&info.encryption), dir.display()),
                Some(&info.file_name),
                serde_json::json!({ "dir": dir.display().to_string(), "encryption": info.encryption, "schemaVersion": info.schema_version }),
            )?;
        }
        Err(e) => {
            history::record(data_dir, "backup", "failed", &format!("{} 백업 실패 ({}): {} {}", kind_label(kind), dir.display(), e.message, e.detail.clone().unwrap_or_default()), None, serde_json::json!({ "dir": dir.display().to_string() }))?;
        }
    }
    result
}

fn create_inner(conn: &Connection, data_dir: &Path, dir: &Path, kind: &str, note: &str, opts: &CreateOpts) -> AppResult<BackupInfo> {
    // 기본 폴더만 자동으로 만든다. 사용자 지정 경로(USB·네트워크)가 없으면 '연결 안 됨' 으로 보고 만들지 않는다.
    if dir == default_dir(data_dir) {
        std::fs::create_dir_all(dir).map_err(|e| io_err("백업 폴더를 만들 수 없습니다 (경로 접근 불가).", e))?;
    }
    let check = check_dir(dir, 0);
    if !check.ok {
        return Err(AppError::new("BACKUP_DIR", check.message));
    }
    let stamp = chrono::Local::now();
    let label = kind_label(kind).split('(').next().unwrap_or("").trim().to_string();
    let ext = if opts.encryption == "none" { "zip" } else { "asbk" };
    let mut base = format!("방과후회계백업_{}_{}", stamp.format("%Y%m%d_%H%M%S"), label);
    let mut n = 1;
    while dir.join(format!("{base}.zip")).exists() || dir.join(format!("{base}.asbk")).exists() {
        n += 1;
        base = format!("방과후회계백업_{}_{}_{}", stamp.format("%Y%m%d_%H%M%S"), label, n);
    }
    let file_name = format!("{base}.{ext}");
    let final_path = dir.join(&file_name);
    let tmp_zip = dir.join(format!(".tmp-{base}.zip"));
    let tmp_final = dir.join(format!(".tmp-{base}.{ext}"));
    let work = tempfile::Builder::new().prefix(".backup-").tempdir_in(data_dir).map_err(|e| io_err("임시 폴더를 만들지 못했습니다.", e))?;
    let outcome = (|| -> AppResult<BackupInfo> {
        // 1. 일관된 DB 스냅샷 (Online Backup API)
        let snap = work.path().join(DB_IN_PACKAGE);
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
        let need: u64 = files.iter().map(|f| f.size).sum::<u64>() + 1_048_576;
        let check = check_dir(dir, need);
        if !check.ok {
            return Err(AppError::new("BACKUP_DIR", check.message));
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
        // 3. zip (임시 파일)
        {
            let f = std::fs::File::create(&tmp_zip).map_err(|e| io_err("백업 파일을 만들 수 없습니다 (경로 접근 불가 또는 쓰기 권한 없음).", e))?;
            let mut z = zip::ZipWriter::new(f);
            let opt = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
            z.start_file("manifest.json", opt).map_err(|e| io_err("패키지를 쓰지 못했습니다.", e))?;
            z.write_all(serde_json::to_string_pretty(&manifest).unwrap_or_default().as_bytes())?;
            for (name, src) in &sources {
                z.start_file(name, opt).map_err(|e| io_err("패키지를 쓰지 못했습니다.", e))?;
                let mut r = std::fs::File::open(src)?;
                std::io::copy(&mut r, &mut z).map_err(|e| io_err("백업 파일을 쓰는 중 연결이 끊기거나 공간이 부족합니다.", e))?;
            }
            z.finish().map_err(|e| io_err("패키지를 닫지 못했습니다.", e))?;
        }
        if opts.fail_at == Some("after_zip") {
            return Err(AppError::new("TEST_FAIL", "시험용 실패 (zip 뒤)"));
        }
        // 4. 암호화
        if opts.encryption != "none" {
            let plain = zeroize::Zeroizing::new(std::fs::read(&tmp_zip)?);
            let mode = match opts.encryption {
                "password" => crypto::Mode::Password(opts.password.ok_or_else(|| AppError::invalid("비밀번호 암호화에는 비밀번호가 필요합니다."))?),
                _ => crypto::Mode::Dpapi,
            };
            let enc = crypto::encrypt(&plain, mode)?;
            std::fs::write(&tmp_final, &enc).map_err(|e| io_err("암호화 백업 파일을 쓰지 못했습니다.", e))?;
            let _ = std::fs::remove_file(&tmp_zip);
            if opts.fail_at == Some("after_encrypt") {
                return Err(AppError::new("TEST_FAIL", "시험용 실패 (암호화 뒤)"));
            }
        }
        // 5. 검증 (임시 파일 상태에서)
        let v = verify_with(&tmp_final, data_dir, opts.password)?;
        if !v.ok {
            return Err(AppError::new("BACKUP_VERIFY", format!("백업 파일을 만들었지만 검증에 실패했습니다: {}", v.errors.join(" / "))));
        }
        // 6. 최종 이름으로 전환 + 곁 파일
        std::fs::rename(&tmp_final, &final_path).map_err(|e| io_err("백업 파일 이름을 바꾸지 못했습니다.", e))?;
        write_sidecar(&final_path, &v)?;
        write_meta(&final_path, &manifest, opts.encryption)?;
        info_of(&final_path)
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(&tmp_zip);
        let _ = std::fs::remove_file(&tmp_final);
    }
    drop(work);
    outcome
}

fn sidecar_path(p: &Path) -> PathBuf {
    PathBuf::from(format!("{}.verify.json", p.display()))
}

fn meta_path(p: &Path) -> PathBuf {
    PathBuf::from(format!("{}.meta.json", p.display()))
}

fn write_sidecar(p: &Path, v: &BackupVerify) -> AppResult<()> {
    let s = serde_json::json!({ "ok": v.ok, "checkedAt": v.checked_at, "errors": v.errors, "warnings": v.warnings });
    std::fs::write(sidecar_path(p), s.to_string())?;
    Ok(())
}

fn read_sidecar(p: &Path) -> Option<(bool, Vec<String>)> {
    let text = std::fs::read_to_string(sidecar_path(p)).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    Some((v["ok"].as_bool().unwrap_or(false), v["errors"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect()).unwrap_or_default()))
}

/// 목록용 메타 — 개인정보 없음 (종류·시각·버전·건수·암호화 방식).
fn write_meta(p: &Path, m: &BackupManifest, encryption: &str) -> AppResult<()> {
    let s = serde_json::json!({
        "kind": m.kind, "createdAt": m.created_at, "appVersion": m.app_version, "schemaVersion": m.schema_version, "years": m.years,
        "complete": m.complete, "fileCount": m.files.len(), "counts": m.counts, "encryption": encryption, "note": m.note,
    });
    std::fs::write(meta_path(p), s.to_string())?;
    Ok(())
}

/// 패키지(zip 또는 asbk)를 읽을 수 있는 zip 경로로 — asbk 면 임시 폴더에 복호화한다. 임시 폴더는 반환된 TempDir 과 함께 지워진다.
fn open_package(path: &Path, work_dir: &Path, password: Option<&str>) -> AppResult<(PathBuf, Option<tempfile::TempDir>, String)> {
    if !crypto::is_encrypted_path(path) {
        // 옛 형식 확인: 내용이 암호화 매직으로 시작하면 확장자가 틀린 것
        let mut head = [0u8; 6];
        let mut f = std::fs::File::open(path)?;
        let n = f.read(&mut head)?;
        if n == 6 && crypto::is_encrypted_file(&head) {
            return Err(AppError::new("BACKUP", "암호화된 백업인데 확장자가 .asbk 가 아닙니다."));
        }
        return Ok((path.to_path_buf(), None, "none".into()));
    }
    let bytes = std::fs::read(path)?;
    let (header, _) = crypto::read_header(&bytes)?;
    let plain = crypto::decrypt(&bytes, password)?;
    std::fs::create_dir_all(work_dir)?;
    let tmp = tempfile::Builder::new().prefix(".backup-open-").tempdir_in(work_dir).map_err(|e| io_err("임시 폴더를 만들지 못했습니다.", e))?;
    let zip_path = tmp.path().join("package.zip");
    std::fs::write(&zip_path, &*plain)?;
    Ok((zip_path, Some(tmp), header.mode))
}

fn manifest_from_zip(zip_path: &Path) -> AppResult<BackupManifest> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| io_err("백업 패키지를 열지 못했습니다.", e))?;
    let mut m = z.by_name("manifest.json").map_err(|e| io_err("백업 패키지에 manifest.json 이 없습니다.", e))?;
    let mut text = String::new();
    m.read_to_string(&mut text)?;
    serde_json::from_str(&text).map_err(|e| io_err("manifest.json 을 읽지 못했습니다.", e))
}

/// manifest 읽기 (암호화 백업은 복호화 필요).
pub fn read_manifest(path: &Path, work_dir: &Path, password: Option<&str>) -> AppResult<BackupManifest> {
    let (zip_path, _tmp, _) = open_package(path, work_dir, password)?;
    manifest_from_zip(&zip_path)
}

/// 패키지 검증 — 형식 확인 → (복호화·인증 태그) → 구조 → 체크섬 → 전체 해시 → SQLite integrity_check → 외래키 → 주요 표 → 메타데이터.
pub fn verify_with(path: &Path, work_dir: &Path, password: Option<&str>) -> AppResult<BackupVerify> {
    let checked_at = history::now();
    let mut warnings = Vec::new();
    if !path.is_file() {
        return Ok(BackupVerify { ok: false, checked_at, errors: vec!["백업 파일이 없습니다.".into()], warnings });
    }
    let (zip_path, _tmp, mode) = match open_package(path, work_dir, password) {
        Ok(x) => x,
        Err(e) => return Ok(BackupVerify { ok: false, checked_at, errors: vec![e.message], warnings }),
    };
    if mode == "none" {
        warnings.push("이 백업 파일은 암호화되지 않았습니다. 학생 개인정보가 들어 있으니 보관·반출에 주의하세요.".into());
    }
    let mut v = verify_zip(&zip_path)?;
    v.warnings.extend(warnings);
    v.checked_at = checked_at;
    Ok(v)
}

/// 비밀번호 없이 검증 (현재 계정용·비암호화).
pub fn verify(path: &Path, work_dir: &Path) -> AppResult<BackupVerify> {
    verify_with(path, work_dir, None)
}

fn verify_zip(zip_path: &Path) -> AppResult<BackupVerify> {
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let checked_at = history::now();
    let manifest = match manifest_from_zip(zip_path) {
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
    let tmp = tempfile::Builder::new().prefix(".backup-verify-").tempdir_in(zip_path.parent().unwrap_or(Path::new("."))).map_err(|e| io_err("임시 폴더를 만들지 못했습니다.", e))?;
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

fn read_meta(p: &Path) -> Option<serde_json::Value> {
    serde_json::from_str(&std::fs::read_to_string(meta_path(p)).ok()?).ok()
}

pub fn info_of(path: &Path) -> AppResult<BackupInfo> {
    let size = std::fs::metadata(path)?.len();
    let file_name = path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let location = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let encrypted = crypto::is_encrypted_path(path);
    let (verified, errs) = read_sidecar(path).unwrap_or((false, vec!["검증 기록이 없습니다.".into()]));
    // 메타 곁 파일 → 없으면(옛 zip) manifest 를 직접 읽는다
    let meta = read_meta(path).or_else(|| {
        if encrypted {
            None
        } else {
            manifest_from_zip(path).ok().map(|m| serde_json::json!({ "kind": m.kind, "createdAt": m.created_at, "appVersion": m.app_version, "schemaVersion": m.schema_version, "years": m.years, "complete": m.complete, "fileCount": m.files.len(), "counts": m.counts, "encryption": "none", "note": m.note }))
        }
    });
    match meta {
        Some(m) => {
            let kind = m["kind"].as_str().unwrap_or("unknown").to_string();
            let encryption = m["encryption"].as_str().unwrap_or(if encrypted { "dpapi" } else { "none" }).to_string();
            Ok(BackupInfo {
                file_name,
                path: path.display().to_string(),
                size,
                kind_label: kind_label(&kind).into(),
                protected: is_protected(&kind),
                kind,
                created_at: m["createdAt"].as_str().unwrap_or("").into(),
                app_version: m["appVersion"].as_str().unwrap_or("").into(),
                schema_version: m["schemaVersion"].as_i64().unwrap_or(0) as i32,
                years: m["years"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default(),
                complete: m["complete"].as_bool().unwrap_or(false),
                file_count: m["fileCount"].as_i64().unwrap_or(0),
                verified,
                verify_errors: errs,
                counts: serde_json::from_value(m["counts"].clone()).ok(),
                legacy: false,
                encrypted,
                encryption_label: encryption_label(&encryption).into(),
                encryption,
                location,
                fallback: false,
            })
        }
        None => Ok(BackupInfo {
            file_name,
            path: path.display().to_string(),
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
            verify_errors: if encrypted { vec!["암호화된 백업인데 목록 정보(.meta.json)가 없습니다. 복원 화면에서 직접 골라 검증하세요.".into()] } else { errs },
            counts: None,
            protected: true,
            legacy: false,
            encrypted,
            encryption_label: encryption_label(if encrypted { "password" } else { "none" }).into(),
            encryption: if encrypted { "password".into() } else { "none".into() },
            location,
            fallback: false,
        }),
    }
}

fn scan_dir(dir: &Path, backups: &mut Vec<BackupInfo>, failed: &mut Vec<BackupInfo>) -> AppResult<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with(".tmp-") || name.ends_with(".json") {
            continue;
        }
        if name.ends_with(".zip") || name.ends_with(".asbk") {
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
                encrypted: false,
                encryption: "none".into(),
                encryption_label: encryption_label("none").into(),
                location: dir.display().to_string(),
                fallback: false,
            });
        }
    }
    Ok(())
}

/// 백업 목록 — 현재 백업 폴더(+ 지정 경로를 쓰는 중이면 기본 폴더도). 검증 통과 / 실패·미검증·이전 형식 구분, 최신 순.
pub fn list(conn: &Connection, data_dir: &Path) -> AppResult<BackupList> {
    let (dir, custom) = resolve_dir(conn, data_dir)?;
    let s = settings(conn)?;
    let mut backups = Vec::new();
    let mut failed = Vec::new();
    scan_dir(&dir, &mut backups, &mut failed)?;
    if custom {
        scan_dir(&default_dir(data_dir), &mut backups, &mut failed)?;
    }
    backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    failed.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    let dir_check = check_dir(&dir, 0);
    Ok(BackupList {
        dir: dir.display().to_string(),
        default_dir: default_dir(data_dir).display().to_string(),
        custom_dir: if custom { Some(dir.display().to_string()) } else { None },
        dir_check,
        last_backup_at: backups.first().map(|b| b.created_at.clone()),
        backups,
        failed,
        auto_enabled: s.auto_enabled,
        keep: s.keep,
        encryption: s.encryption,
        fallback_enabled: s.fallback,
    })
}

/// 자동 백업 정리 — 검증된 자동 백업 중 최근 `keep` 개만 남긴다. 보호 종류는 건드리지 않는다. 삭제 내역은 이력에 남긴다.
pub fn cleanup(data_dir: &Path, dir: &Path, keep: i64) -> AppResult<Vec<String>> {
    let mut autos: Vec<BackupInfo> = Vec::new();
    if dir.is_dir() {
        for e in std::fs::read_dir(dir)? {
            let p = e?.path();
            let ext = p.extension().map(|x| x.to_string_lossy().to_string()).unwrap_or_default();
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if (ext == "zip" || ext == "asbk") && !name.starts_with(".tmp-") {
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
        let _ = std::fs::remove_file(meta_path(&p));
        history::record(data_dir, "cleanup", "ok", &format!("자동 백업 정리: {} 삭제 (보관 {}개 초과)", b.file_name, keep), Some(&b.file_name), serde_json::json!({ "reason": "retention", "keep": keep, "dir": dir.display().to_string() }))?;
        removed.push(b.file_name);
    }
    Ok(removed)
}

/// 비정상 종료로 남은 임시 파일·폴더 정리 (시작 때). 이름이 `.tmp-`·`.backup-`·`.restore-` 로 시작하는 것만.
pub fn cleanup_temp(dirs: &[PathBuf]) -> Vec<String> {
    let mut removed = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(dir) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            let name = p.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with(".tmp-") || name.starts_with(".backup-") || name.starts_with(".restore-") || name.starts_with(".write-test-") {
                let ok = if p.is_dir() { std::fs::remove_dir_all(&p).is_ok() } else { std::fs::remove_file(&p).is_ok() };
                if ok {
                    removed.push(p.display().to_string());
                }
            }
        }
    }
    removed
}

/// 정기(하루 한 번) 자동 백업 — 프로그램 시작 때 부른다. 오늘 자동 백업이 있으면 건너뛴다. 지정 경로 실패는 기록하고 대체한다.
pub fn auto_daily(conn: &Connection, data_dir: &Path) -> AppResult<Option<BackupInfo>> {
    let s = settings(conn)?;
    if !s.auto_enabled {
        return Ok(None);
    }
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let l = list(conn, data_dir)?;
    if l.backups.iter().any(|b| b.kind == "auto" && b.created_at.starts_with(&today)) {
        return Ok(None);
    }
    let info = create(conn, data_dir, "auto", "정기 자동 백업")?;
    let (dir, _) = resolve_dir(conn, data_dir)?;
    cleanup(data_dir, &dir, s.keep)?;
    if info.fallback {
        cleanup(data_dir, &default_dir(data_dir), s.keep)?;
    }
    Ok(Some(info))
}
