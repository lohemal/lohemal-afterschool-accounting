use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{fundalias, program, upload, year};

#[tauri::command]
pub fn upload_inspect(db: State<Db>, school_year_id: i64, path: String) -> AppResult<Inspect> {
    db.read(|c| upload::inspect(c, school_year_id, std::path::Path::new(&path)))
}

#[tauri::command]
pub fn upload_preview(db: State<Db>, req: PreviewRequest) -> AppResult<Preview> {
    db.read(|c| upload::preview(c, &req))
}

#[tauri::command]
pub fn upload_commit(db: State<Db>, req: PreviewRequest) -> AppResult<CommitResult> {
    let dir = db.data_dir();
    db.write(|tx| upload::commit(tx, &dir, &req))
}

#[tauri::command]
pub fn upload_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<UploadFile>> {
    db.read(|c| upload::list(c, school_year_id))
}

#[tauri::command]
pub fn upload_detail(db: State<Db>, file_id: i64) -> AppResult<UploadDetail> {
    db.read(|c| upload::detail(c, file_id))
}

#[tauri::command]
pub fn upload_impact(db: State<Db>, file_id: i64, action: String) -> AppResult<Impact> {
    db.read(|c| upload::impact(c, file_id, &action))
}

#[tauri::command]
pub fn upload_cancel(db: State<Db>, file_id: i64, reason: String) -> AppResult<UploadFile> {
    db.write(|tx| upload::cancel(tx, file_id, &reason))
}

#[tauri::command]
pub fn upload_restore(db: State<Db>, file_id: i64, reason: String) -> AppResult<UploadFile> {
    db.write(|tx| upload::restore(tx, file_id, &reason))
}

#[tauri::command]
pub fn upload_kinds() -> Vec<(String, String)> {
    upload::KINDS.iter().map(|(c, l)| (c.to_string(), l.to_string())).collect()
}

#[tauri::command]
pub fn program_map_save(db: State<Db>, input: ProgramMapInput) -> AppResult<()> {
    db.write(|tx| program::map_save(tx, &input).map(|_| ()))
}

#[tauri::command]
pub fn program_group_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<ProgramGroup>> {
    db.read(|c| program::groups(c, school_year_id))
}

#[tauri::command]
pub fn program_alias_delete(db: State<Db>, school_year_id: i64, alias: String) -> AppResult<()> {
    db.write(|tx| program::delete_alias(tx, school_year_id, &alias))
}

#[tauri::command]
pub fn fund_alias_list(db: State<Db>) -> AppResult<Vec<FundAlias>> {
    db.read(fundalias::list)
}

#[tauri::command]
pub fn fund_alias_save(db: State<Db>, fund_type_id: i64, alias: String) -> AppResult<FundAlias> {
    db.write(|tx| fundalias::save(tx, fund_type_id, &alias))
}

#[tauri::command]
pub fn fund_alias_delete(db: State<Db>, id: i64) -> AppResult<()> {
    db.write(|tx| fundalias::delete(tx, id))
}

#[tauri::command]
pub fn month_close(db: State<Db>, fiscal_month_id: i64, note: Option<String>) -> AppResult<()> {
    db.write(|tx| year::month_close(tx, fiscal_month_id, note.as_deref().unwrap_or("")))
}

#[tauri::command]
pub fn month_reopen(db: State<Db>, fiscal_month_id: i64, note: String) -> AppResult<()> {
    if note.trim().is_empty() {
        return Err(AppError::invalid("재개 사유를 입력해 주세요."));
    }
    db.write(|tx| year::month_reopen(tx, fiscal_month_id, &note))
}
