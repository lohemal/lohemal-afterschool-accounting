use tauri::State;

use crate::db::{migrate, Db};
use crate::error::{AppError, AppResult};
use crate::model::{AppInfo, AuditLog, Bootstrap, Counts};
use crate::repo::{audit, setting, year};

fn app_info(db: &Db) -> AppResult<AppInfo> {
    let schema_version = db.read(|c| Ok(migrate::current_version(c)?))?;
    Ok(AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        db_path: db.path().display().to_string(),
        data_dir: db.data_dir().display().to_string(),
        schema_version,
    })
}

#[tauri::command]
pub fn bootstrap(db: State<Db>) -> AppResult<Bootstrap> {
    let app = app_info(&db)?;
    db.read(|c| {
        let years = year::list(c)?;
        let current_year = year::current(c)?;
        let school_name = setting::get(c, "school_name")?.unwrap_or_default();
        let counts = match &current_year {
            Some(y) => Counts {
                programs: c.query_row("SELECT count(*) FROM program WHERE school_year_id = ?1", [y.id], |r| r.get(0))?,
                uploads: c.query_row(
                    "SELECT count(*) FROM upload_file WHERE school_year_id = ?1 AND status NOT IN ('cancelled','replaced')",
                    [y.id],
                    |r| r.get(0),
                )?,
                entries: c.query_row(
                    "SELECT count(*) FROM ledger_entry WHERE school_year_id = ?1 AND state = 'active'",
                    [y.id],
                    |r| r.get(0),
                )?,
                students: c.query_row("SELECT count(*) FROM student WHERE school_year_id = ?1", [y.id], |r| r.get(0))?,
            },
            None => Counts { programs: 0, uploads: 0, entries: 0, students: 0 },
        };
        Ok(Bootstrap { app, years, current_year, school_name, counts })
    })
}

#[tauri::command]
pub fn setting_get(db: State<Db>, key: String) -> AppResult<Option<String>> {
    db.read(|c| setting::get(c, &key))
}

#[tauri::command]
pub fn setting_set(db: State<Db>, key: String, value: String) -> AppResult<()> {
    if key.trim().is_empty() {
        return Err(AppError::invalid("설정 키가 비어 있습니다."));
    }
    db.write(|tx| {
        setting::set(tx, &key, value.trim())?;
        audit::write(tx, "setting.set", "app_setting", None, &format!("설정 '{key}' 변경"), serde_json::json!({}))?;
        Ok(())
    })
}

#[tauri::command]
pub fn audit_list(db: State<Db>, limit: Option<i64>) -> AppResult<Vec<AuditLog>> {
    db.read(|c| audit::list(c, limit.unwrap_or(200).clamp(1, 2000)))
}

/// 자료 폴더를 탐색기로 연다 (DB·로그·백업 위치 확인용).
#[tauri::command]
pub fn open_data_folder(db: State<Db>) -> AppResult<()> {
    let dir = db.data_dir();
    tauri_plugin_opener::open_path(dir, None::<&str>)
        .map_err(|e| AppError::new("OPEN", "자료 폴더를 열지 못했습니다.").detail(e.to_string()))
}
