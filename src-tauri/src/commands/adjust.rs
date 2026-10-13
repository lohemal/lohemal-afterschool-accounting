use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::*;
use crate::repo::adjustment;

#[tauri::command]
pub fn adjustment_preview(db: State<Db>, input: AdjustmentInput) -> AppResult<AdjustmentPreview> {
    db.read(|c| adjustment::preview(c, &input))
}

#[tauri::command]
pub fn adjustment_create(db: State<Db>, input: AdjustmentInput) -> AppResult<Adjustment> {
    db.write(|tx| adjustment::create(tx, &input))
}

#[tauri::command]
pub fn adjustment_list(db: State<Db>, filter: AdjustmentFilter) -> AppResult<Vec<Adjustment>> {
    db.read(|c| adjustment::list(c, &filter))
}

#[tauri::command]
pub fn adjustment_set_status(db: State<Db>, id: i64, status: String, processed_on: Option<String>, note: Option<String>) -> AppResult<Adjustment> {
    db.write(|tx| adjustment::set_status(tx, id, &status, processed_on.as_deref(), note.as_deref().unwrap_or("")))
}

#[tauri::command]
pub fn adjustment_cancel(db: State<Db>, id: i64, reason: String) -> AppResult<Adjustment> {
    db.write(|tx| adjustment::cancel(tx, id, &reason))
}

#[tauri::command]
pub fn student_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<StudentRow>> {
    db.read(|c| adjustment::students(c, school_year_id))
}

#[tauri::command]
pub fn entry_picks(db: State<Db>, fiscal_month_id: i64, program_id: i64, student_id: Option<i64>) -> AppResult<Vec<EntryPick>> {
    db.read(|c| adjustment::entry_picks(c, fiscal_month_id, program_id, student_id))
}
