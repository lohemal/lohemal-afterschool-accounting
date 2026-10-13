//! P7-1.1 — 세부반 → 통합 프로그램 연결.

use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::*;
use crate::repo::program_link;

#[tauri::command]
pub fn program_link_board(db: State<Db>, school_year_id: i64) -> AppResult<LinkBoard> {
    db.read(|c| program_link::board(c, school_year_id))
}

#[tauri::command]
pub fn program_link_preview(db: State<Db>, school_year_id: i64, items: Vec<LinkItem>) -> AppResult<LinkPreview> {
    db.read(|c| program_link::preview(c, school_year_id, &items))
}

#[tauri::command]
pub fn program_link_apply(db: State<Db>, school_year_id: i64, items: Vec<LinkItem>, note: Option<String>, rule: Option<String>) -> AppResult<LinkApplyResult> {
    db.write(|tx| program_link::apply(tx, school_year_id, &items, note.as_deref().unwrap_or(""), rule.as_deref().unwrap_or("manual")))
}

#[tauri::command]
pub fn program_umbrella_create(db: State<Db>, school_year_id: i64, name: String, member_ids: Vec<i64>, note: Option<String>) -> AppResult<i64> {
    db.write(|tx| program_link::umbrella_create(tx, school_year_id, &name, &member_ids, note.as_deref().unwrap_or("")))
}

#[tauri::command]
pub fn program_link_history(db: State<Db>, school_year_id: i64) -> AppResult<Vec<LinkLog>> {
    db.read(|c| program_link::history(c, school_year_id))
}
