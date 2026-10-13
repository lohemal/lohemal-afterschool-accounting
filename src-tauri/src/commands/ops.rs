use std::path::Path;

use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::ops::{backup, export, history, restore};
use crate::repo::{integrity, setting};

#[tauri::command]
pub fn backup_list(db: State<Db>) -> AppResult<BackupList> {
    let dir = db.data_dir();
    db.read(|c| backup::list(c, &dir))
}

/// 수동 백업. 비밀번호를 주면 다른 PC 이전용(비밀번호 암호화), 없으면 설정의 기본 방식(현재 계정용 또는 암호화 안 함).
#[tauri::command]
pub fn backup_create(db: State<Db>, note: Option<String>, password: Option<String>) -> AppResult<BackupInfo> {
    let dir = db.data_dir();
    let pw = password.as_deref().map(str::trim).filter(|s| !s.is_empty());
    db.read(|c| backup::create_with_password(c, &dir, "manual", note.as_deref().unwrap_or(""), pw))
}

/// 업데이트 직전 안전 백업 (화면의 업데이트 설치 전에 부른다).
#[tauri::command]
pub fn backup_before_update(db: State<Db>, new_version: String) -> AppResult<BackupInfo> {
    let dir = db.data_dir();
    db.read(|c| backup::create(c, &dir, "pre_update", &format!("v{} 업데이트 직전", new_version.trim())))
}

#[tauri::command]
pub fn backup_settings_set(db: State<Db>, input: BackupSettingsInput) -> AppResult<()> {
    if !(1..=100).contains(&input.keep) {
        return Err(AppError::invalid("보관 개수는 1~100 이어야 합니다."));
    }
    if !matches!(input.encryption.as_str(), "none" | "dpapi") {
        return Err(AppError::invalid("암호화 방식이 올바르지 않습니다."));
    }
    let custom = input.custom_dir.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from);
    if let Some(c) = &custom {
        let chk = backup::check_dir(Path::new(c), 0);
        if !chk.ok {
            return Err(AppError::invalid(format!("지정 백업 경로를 쓸 수 없습니다: {}", chk.message)));
        }
    }
    db.write(|tx| {
        setting::set(tx, "backup.auto", if input.auto_enabled { "1" } else { "0" })?;
        setting::set(tx, "backup.keep", &input.keep.to_string())?;
        setting::set(tx, "backup.dir", custom.as_deref().unwrap_or(""))?;
        setting::set(tx, "backup.encrypt", &input.encryption)?;
        setting::set(tx, "backup.fallback", if input.fallback_enabled { "1" } else { "0" })?;
        crate::repo::audit::write(tx, "setting.backup", "app_setting", None, &format!("백업 설정: 자동 {} · 보관 {}개 · {} · 경로 {}", if input.auto_enabled { "켬" } else { "끔" }, input.keep, backup::encryption_label(&input.encryption), if custom.is_some() { "사용자 지정" } else { "기본" }), serde_json::json!({}))?;
        Ok(())
    })?;
    let dir = db.data_dir();
    let (bdir, _) = db.read(|c| backup::resolve_dir(c, &dir))?;
    backup::cleanup(&dir, &bdir, input.keep)?;
    Ok(())
}

#[tauri::command]
pub fn backup_dir_check(path: String) -> AppResult<DirCheck> {
    Ok(backup::check_dir(Path::new(path.trim()), 50 * 1_048_576))
}

#[tauri::command]
pub fn backup_verify(db: State<Db>, path: String, password: Option<String>) -> AppResult<BackupVerify> {
    let dir = db.data_dir();
    backup::verify_with(Path::new(&path), &dir, password.as_deref().filter(|s| !s.is_empty()))
}

#[tauri::command]
pub fn restore_inspect(db: State<Db>, path: String, password: Option<String>) -> AppResult<RestorePreview> {
    restore::inspect(&db, Path::new(&path), password.as_deref().filter(|s| !s.is_empty()))
}

#[tauri::command]
pub fn restore_apply(db: State<Db>, path: String, confirm: bool, password: Option<String>) -> AppResult<RestoreResult> {
    restore::apply_with_password(&db, Path::new(&path), confirm, password.as_deref().filter(|s| !s.is_empty()))
}

#[tauri::command]
pub fn month_changes(db: State<Db>, fiscal_month_id: i64) -> AppResult<MonthChanges> {
    db.read(|c| crate::repo::year::month_changes(c, fiscal_month_id))
}

#[tauri::command]
pub fn integrity_run(db: State<Db>, school_year_id: i64) -> AppResult<IntegrityReport> {
    let dir = db.data_dir();
    let rep = db.read(|c| integrity::run(c, &dir, school_year_id))?;
    history::record(&dir, "integrity", if rep.error_count > 0 { "failed" } else { "ok" }, &format!("무결성 검사: 정상 {} · 확인 필요 {} · 오류 {}", rep.ok_count, rep.check_count, rep.error_count), None, serde_json::json!({ "schoolYearId": school_year_id }))?;
    Ok(rep)
}

#[tauri::command]
pub fn ops_history(db: State<Db>, limit: Option<usize>) -> AppResult<Vec<OpsRecord>> {
    history::list(&db.data_dir(), limit.unwrap_or(200))
}

#[tauri::command]
pub fn export_recon(db: State<Db>, fiscal_month_id: i64, filter: ReconExportFilter, path: String) -> AppResult<ExportResult> {
    let dir = db.data_dir();
    db.read(|c| export::recon_month(c, &dir, fiscal_month_id, &filter, Path::new(&path)))
}

#[tauri::command]
pub fn export_voucher_usage(db: State<Db>, school_year_id: i64, path: String) -> AppResult<ExportResult> {
    let dir = db.data_dir();
    db.read(|c| export::voucher_usage(c, &dir, school_year_id, Path::new(&path)))
}

#[tauri::command]
pub fn export_voucher_refunds(db: State<Db>, school_year_id: i64, filter: RefundExportFilter, path: String) -> AppResult<ExportResult> {
    let dir = db.data_dir();
    db.read(|c| export::voucher_refunds(c, &dir, school_year_id, &filter, Path::new(&path)))
}

#[tauri::command]
pub fn export_integrity(db: State<Db>, school_year_id: i64, path: String) -> AppResult<ExportResult> {
    let dir = db.data_dir();
    db.read(|c| export::integrity_report(c, &dir, school_year_id, Path::new(&path)))
}

#[tauri::command]
pub fn export_default_name(db: State<Db>, kind: String, school_year_id: i64, fiscal_month_id: Option<i64>) -> AppResult<String> {
    db.read(|c| {
        let y = crate::repo::year::get(c, school_year_id)?;
        let tag = export::today_tag();
        let month = match fiscal_month_id {
            Some(m) => crate::repo::year::month_label_of(c, m)?.split(' ').next().unwrap_or("").to_string(),
            None => String::new(),
        };
        Ok(match kind.as_str() {
            "recon" => format!("방과후회계_월별검증_{}_{}_{tag}.xlsx", y.name, month),
            "usage" => format!("방과후회계_자유수강권사용액_{}_{tag}.xlsx", y.name),
            "refunds" => format!("방과후회계_자유수강권환불_{}_{tag}.xlsx", y.name),
            _ => format!("방과후회계_무결성검사_{}_{tag}.xlsx", y.name),
        })
    })
}
