//! 복원 — 백업 패키지를 골랐다고 바로 덮어쓰지 않는다.
//!
//!   검증 → 메타데이터·현재 자료 비교(미리보기) → 담당자 최종 확인(`confirm`) → 복원 직전 안전 백업 → 임시 위치에 준비·검증 →
//!   연결을 닫고(잠금 유지, 다른 쓰기 차단) 파일 전환 → 다시 열어 무결성 검사 → 실패하면 원래 파일로 되돌린다.
//!   복원 이력은 DB 밖(ops-history.jsonl)에 남아 과거 DB 를 복원해도 사라지지 않는다. 부분 복원은 하지 않는다(전체 복원만).

use std::io::Read;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::db::{migrate, Db};
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::ops::{backup, history};

/// 패키지(zip/asbk)를 읽을 수 있는 zip 으로 연다. asbk 면 자료 폴더 안 임시 폴더에 복호화한다.
fn open_zip(db: &Db, path: &Path, password: Option<&str>) -> AppResult<(PathBuf, Option<tempfile::TempDir>)> {
    if !crate::ops::crypto::is_encrypted_path(path) {
        return Ok((path.to_path_buf(), None));
    }
    let bytes = std::fs::read(path)?;
    let plain = crate::ops::crypto::decrypt(&bytes, password)?;
    let tmp = tempfile::Builder::new().prefix(".restore-open-").tempdir_in(db.data_dir())?;
    let zp = tmp.path().join("package.zip");
    std::fs::write(&zp, &*plain)?;
    Ok((zp, Some(tmp)))
}

fn extract_to(zip_path: &Path, name: &str, dest: &Path) -> AppResult<()> {
    let f = std::fs::File::open(zip_path)?;
    let mut z = zip::ZipArchive::new(f).map_err(|e| AppError::new("RESTORE", "백업 패키지를 열지 못했습니다.").detail(e.to_string()))?;
    let mut entry = z.by_name(name).map_err(|e| AppError::new("RESTORE", format!("백업에 '{name}' 이(가) 없습니다.")).detail(e.to_string()))?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut out = std::fs::File::create(dest)?;
    std::io::copy(&mut entry, &mut out)?;
    Ok(())
}

fn db_sha(path: &Path) -> String {
    use sha2::Digest;
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let mut h = sha2::Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        match f.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => h.update(&buf[..n]),
        }
    }
    hex::encode(h.finalize())
}

/// 복원 미리보기 — 검증 결과, 백업 메타데이터, 현재 자료와의 건수 비교, 차단 사유.
pub fn inspect(db: &Db, zip_path: &Path, password: Option<&str>) -> AppResult<RestorePreview> {
    let current = db.read(backup::counts)?;
    let data_dir = db.data_dir();
    let verify = backup::verify_with(zip_path, &data_dir, password)?;
    let mut blocked: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if !verify.ok {
        blocked.push(format!("백업 검증 실패: {}", verify.errors.join(" / ")));
    }
    let manifest = if verify.ok { backup::read_manifest(zip_path, &data_dir, password).ok() } else { None };
    let (zip_path_open, _open_tmp) = if verify.ok { open_zip(db, zip_path, password)? } else { (zip_path.to_path_buf(), None) };
    let zip_path = zip_path_open.as_path();
    let mut bcounts = None;
    if let Some(m) = &manifest {
        if m.format_version > backup::FORMAT_VERSION {
            blocked.push(format!("지원하지 않는 백업 형식 v{} 입니다.", m.format_version));
        }
        if m.schema_version > migrate::latest_version() {
            blocked.push(format!("백업의 스키마(v{})가 이 프로그램(v{})보다 새롭습니다. 프로그램을 업데이트한 뒤 복원해 주세요.", m.schema_version, migrate::latest_version()));
        } else if m.schema_version < migrate::latest_version() {
            warnings.push(format!("백업의 스키마는 v{} 입니다. 복원 뒤 v{} 로 자동 갱신됩니다(갱신 직전 백업이 따로 남습니다).", m.schema_version, migrate::latest_version()));
        }
        if !m.complete {
            warnings.push(format!("원본 파일 {}개가 백업에 없어 복원 뒤 그 파일은 '원본 없음' 으로 표시됩니다.", m.missing_files.len()));
        }
        if verify.ok {
            // 백업 DB 의 건수 (검증된 패키지에서만)
            let tmp = tempfile::tempdir()?;
            let p = tmp.path().join("peek.db");
            extract_to(zip_path, backup::DB_IN_PACKAGE, &p)?;
            let c = Connection::open_with_flags(&p, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
            let cn = backup::counts(&c)?;
            if cn.ledger_entries < current.ledger_entries || cn.adjustments < current.adjustments || cn.voucher_targets < current.voucher_targets || cn.audit_logs < current.audit_logs {
                warnings.push("복원하면 백업 이후에 등록한 현재 자료(원장 거래·정정·대상자·감사 이력)가 사라집니다. 복원 직전에 현재 자료의 안전 백업을 만듭니다.".into());
            }
            bcounts = Some(cn);
        }
    }
    warnings.extend(verify.warnings.clone());
    let can_restore = blocked.is_empty();
    Ok(RestorePreview { path: zip_path.display().to_string(), manifest, verify, current, backup: bcounts, blocked, warnings, can_restore })
}

pub fn apply_with_password(db: &Db, zip_path: &Path, confirm: bool, password: Option<&str>) -> AppResult<RestoreResult> {
    apply_inner(db, zip_path, confirm, None, password)
}

/// 복원 실행. `confirm` 이 없으면 아무것도 하지 않는다.
pub fn apply(db: &Db, zip_path: &Path, confirm: bool) -> AppResult<RestoreResult> {
    apply_inner(db, zip_path, confirm, None, None)
}

/// `fail_at` 은 시험용 — 그 지점에서 일부러 실패시켜 원상 복구를 확인한다.
pub fn apply_inner(db: &Db, package_path: &Path, confirm: bool, fail_at: Option<&str>, password: Option<&str>) -> AppResult<RestoreResult> {
    let data_dir = db.data_dir();
    if !confirm {
        return Err(AppError::invalid("복원은 미리보기를 확인하고 최종 확인을 해야 실행됩니다."));
    }
    let pv = inspect(db, package_path, password)?;
    let file_name = package_path.file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let (zip_buf, _open_tmp) = if pv.can_restore { open_zip(db, package_path, password)? } else { (package_path.to_path_buf(), None) };
    let zip_path = zip_buf.as_path();
    if !pv.can_restore {
        history::record(&data_dir, "restore", "blocked", &format!("복원 차단: {}", pv.blocked.join(" / ")), Some(&file_name), serde_json::json!({ "blocked": pv.blocked }))?;
        return Err(AppError::invalid(format!("복원할 수 없습니다: {}", pv.blocked.join(" / "))));
    }
    let before = pv.current.clone();
    // 1. 복원 직전 안전 백업 (검증까지 통과해야 진행)
    let safety = db.read(|c| backup::create(c, &data_dir, "pre_restore", &format!("'{file_name}' 복원 직전")))?;
    let before_sha = db_sha(Path::new(&safety.path)).to_string();
    let before_db_sha = {
        // 안전 백업 안의 DB 해시 = 복원 전 DB 식별자
        backup::read_manifest(Path::new(&safety.path), &data_dir, None).ok().and_then(|m| m.files.iter().find(|f| f.path == backup::DB_IN_PACKAGE).map(|f| f.sha256.clone())).unwrap_or(before_sha)
    };
    let after_db_sha = pv.manifest.as_ref().and_then(|m| m.files.iter().find(|f| f.path == backup::DB_IN_PACKAGE).map(|f| f.sha256.clone())).unwrap_or_default();
    // 2. 임시 위치에 준비 + 검증
    let staging = tempfile::Builder::new().prefix(".restore-").tempdir_in(&data_dir)?;
    let staged_db = staging.path().join("restore.db");
    extract_to(zip_path, backup::DB_IN_PACKAGE, &staged_db)?;
    {
        let c = Connection::open_with_flags(&staged_db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let ic: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
        if ic != "ok" {
            return Err(AppError::new("RESTORE", format!("준비한 DB 의 무결성 검사 실패: {ic}")));
        }
    }
    let manifest = pv.manifest.clone().ok_or_else(|| AppError::new("RESTORE", "백업 메타데이터가 없습니다."))?;
    // 3. 파일 전환 (잠금 유지 — 다른 읽기·쓰기 차단)
    let aside = PathBuf::from(format!("{}.before-restore", db.path().display()));
    let db_path = db.path().to_path_buf();
    let swap = db.with_file_swap(|path| {
        for suffix in ["-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
        }
        if aside.exists() {
            std::fs::remove_file(&aside)?;
        }
        std::fs::rename(path, &aside)?;
        if fail_at == Some("after_move") {
            return Err(AppError::new("TEST_FAIL", "시험용 실패 (파일 이동 뒤)"));
        }
        std::fs::copy(&staged_db, path)?;
        if fail_at == Some("after_copy") {
            // 깨진 파일을 흉내 낸다
            std::fs::write(path, b"corrupted")?;
        }
        // 원본 파일 — 내용 주소라 같은 이름은 건너뛴다
        let uploads = crate::repo::upload::uploads_dir(&data_dir);
        for f in manifest.files.iter().filter(|f| f.path.starts_with("uploads/")) {
            let name = &f.path["uploads/".len()..];
            let dest = uploads.join(name);
            if !dest.exists() {
                extract_to(zip_path, &f.path, &dest)?;
            }
        }
        Ok(())
    });
    // 4. 결과 판정 — 실패하면 원래 파일 복구
    let mut failure: Option<String> = match swap {
        Ok(()) => None,
        Err(e) => Some(format!("{} {}", e.message, e.detail.clone().unwrap_or_default())),
    };
    if failure.is_none() {
        // 복원 후 무결성 검사
        let check = db.read(|c| {
            let ic: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
            if ic != "ok" {
                return Err(AppError::new("RESTORE", format!("복원 뒤 무결성 검사 실패: {ic}")));
            }
            let fk = c.prepare("PRAGMA foreign_key_check")?.query_map([], |_| Ok(()))?.count();
            if fk > 0 {
                return Err(AppError::new("RESTORE", format!("복원 뒤 외래 키 위반 {fk}건")));
            }
            if fail_at == Some("after_verify") {
                return Err(AppError::new("TEST_FAIL", "시험용 실패 (검증 뒤)"));
            }
            Ok(())
        });
        if let Err(e) = check {
            failure = Some(e.message);
        }
    }
    if let Some(reason) = failure {
        // 원상 복구: 현재 파일(있으면) 제거 → 옆으로 둔 원래 파일 되돌림 → 다시 열기
        let rollback = db.with_file_swap(|path| {
            for suffix in ["-wal", "-shm"] {
                let _ = std::fs::remove_file(format!("{}{suffix}", path.display()));
            }
            if path.exists() {
                std::fs::remove_file(path)?;
            }
            if aside.exists() {
                std::fs::rename(&aside, path)?;
            }
            Ok(())
        });
        let restored_ok = rollback.is_ok() || db.reopen().is_ok();
        let after = db.read(backup::counts).unwrap_or_default();
        history::record(
            &data_dir,
            "restore",
            "failed",
            &format!("복원 실패 — 기존 자료 {}: {reason}", if restored_ok { "유지" } else { "복구 실패(안전 백업으로 복원 필요)" }),
            Some(&file_name),
            serde_json::json!({ "safetyBackup": safety.file_name, "beforeDbSha256": before_db_sha, "reason": reason }),
        )?;
        return Ok(RestoreResult { ok: false, message: format!("복원에 실패해 기존 자료를 그대로 두었습니다: {reason}"), safety_backup: Some(safety.file_name), before: before.clone(), after, before_db_sha256: before_db_sha, after_db_sha256: String::new() });
    }
    let _ = std::fs::remove_file(&aside);
    let after = db.read(backup::counts)?;
    db.write(|tx| {
        crate::repo::audit::write(tx, "ops.restore", "", None, &format!("백업 '{file_name}' 에서 전체 복원 (복원 전 안전 백업 {})", safety.file_name), serde_json::json!({ "afterDbSha256": after_db_sha }))?;
        Ok(())
    })?;
    history::record(
        &data_dir,
        "restore",
        "ok",
        &format!("'{file_name}' 복원 완료 (원장 {}건 → {}건, 안전 백업 {})", before.ledger_entries, after.ledger_entries, safety.file_name),
        Some(&file_name),
        serde_json::json!({ "safetyBackup": safety.file_name, "beforeDbSha256": before_db_sha, "afterDbSha256": after_db_sha, "before": before, "after": after, "dbPath": db_path.display().to_string() }),
    )?;
    Ok(RestoreResult { ok: true, message: "복원이 끝났습니다.".into(), safety_backup: Some(safety.file_name), before, after, before_db_sha256: before_db_sha, after_db_sha256: after_db_sha })
}
