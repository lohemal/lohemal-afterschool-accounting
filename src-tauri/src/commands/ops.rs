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

#[tauri::command]
pub fn backup_create(db: State<Db>, note: Option<String>) -> AppResult<BackupInfo> {
    let dir = db.data_dir();
    db.read(|c| backup::create(c, &dir, "manual", note.as_deref().unwrap_or("")))
}

#[tauri::command]
pub fn backup_settings_set(db: State<Db>, auto_enabled: bool, keep: i64) -> AppResult<()> {
    if !(1..=100).contains(&keep) {
        return Err(AppError::invalid("보관 개수는 1~100 이어야 합니다."));
    }
    db.write(|tx| {
        setting::set(tx, "backup.auto", if auto_enabled { "1" } else { "0" })?;
        setting::set(tx, "backup.keep", &keep.to_string())?;
        crate::repo::audit::write(tx, "setting.backup", "app_setting", None, &format!("자동 백업 {} · 보관 {}개", if auto_enabled { "켬" } else { "끔" }, keep), serde_json::json!({}))?;
        Ok(())
    })?;
    let dir = db.data_dir();
    backup::cleanup(&dir, keep)?;
    Ok(())
}

#[tauri::command]
pub fn backup_verify(path: String) -> AppResult<BackupVerify> {
    backup::verify(Path::new(&path))
}

#[tauri::command]
pub fn restore_inspect(db: State<Db>, path: String) -> AppResult<RestorePreview> {
    restore::inspect(&db, Path::new(&path))
}

#[tauri::command]
pub fn restore_apply(db: State<Db>, path: String, confirm: bool) -> AppResult<RestoreResult> {
    restore::apply(&db, Path::new(&path), confirm)
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
