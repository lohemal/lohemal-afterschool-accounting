use tauri::State;

use crate::db::Db;
use crate::error::{AppError, AppResult};
use crate::model::*;
use crate::repo::{allocation, recon, upload};

#[tauri::command]
pub fn allocation_board(db: State<Db>, fiscal_month_id: i64) -> AppResult<AllocationBoard> {
    db.read(|c| allocation::pending(c, fiscal_month_id))
}

#[tauri::command]
pub fn allocation_auto(db: State<Db>, fiscal_month_id: i64) -> AppResult<AutoAllocateResult> {
    db.write(|tx| allocation::auto_allocate(tx, fiscal_month_id))
}

#[tauri::command]
pub fn allocation_manual(db: State<Db>, entry_id: i64, parts: Vec<ManualPart>, reason: String) -> AppResult<Vec<i64>> {
    let p: Vec<(i64, i64)> = parts.iter().map(|x| (x.program_id, x.amount)).collect();
    db.write(|tx| allocation::manual(tx, entry_id, &p, &reason))
}

#[tauri::command]
pub fn allocation_void(db: State<Db>, allocation_id: i64, reason: String) -> AppResult<()> {
    db.write(|tx| allocation::void(tx, allocation_id, &reason))
}

#[tauri::command]
pub fn recon_month(db: State<Db>, fiscal_month_id: i64) -> AppResult<MonthRecon> {
    db.read(|c| recon::month(c, fiscal_month_id))
}

#[tauri::command]
pub fn recon_detail(db: State<Db>, fiscal_month_id: i64, unit_kind: String, unit_id: i64, item_set: String) -> AppResult<ReconDetail> {
    db.read(|c| recon::detail(c, fiscal_month_id, &unit_kind, unit_id, &item_set))
}

#[tauri::command]
pub fn recon_note_save(db: State<Db>, fiscal_month_id: i64, unit_kind: String, unit_id: i64, item_set: String, status: String, reason: String) -> AppResult<ReconNote> {
    db.write(|tx| recon::note_save(tx, fiscal_month_id, &unit_kind, unit_id, &item_set, &status, &reason))
}

#[tauri::command]
pub fn source_row_view(db: State<Db>, source_row_id: i64) -> AppResult<SourceRowView> {
    db.read(|c| recon::source_row(c, source_row_id))
}

/// 보관된 원본 파일을 연다 (읽기용 — 앱은 파일을 바꾸지 않는다).
#[tauri::command]
pub fn open_stored_file(db: State<Db>, file_id: i64) -> AppResult<()> {
    let f = db.read(|c| upload::get(c, file_id))?;
    if f.stored_name.is_empty() {
        return Err(AppError::not_found("보관된 원본이 없습니다."));
    }
    let path = upload::uploads_dir(&db.data_dir()).join(&f.stored_name);
    if !path.exists() {
        return Err(AppError::not_found("보관된 원본 파일을 찾지 못했습니다."));
    }
    tauri_plugin_opener::open_path(path, None::<&str>)
        .map_err(|e| AppError::new("OPEN", "원본 파일을 열지 못했습니다.").detail(e.to_string()))
}
