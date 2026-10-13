use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::{MonthSummary, Program};
use crate::repo::{ledger, program};

#[tauri::command]
pub fn month_summary(db: State<Db>, fiscal_month_id: i64) -> AppResult<MonthSummary> {
    db.read(|c| ledger::month_summary(c, fiscal_month_id))
}

#[tauri::command]
pub fn program_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<Program>> {
    db.read(|c| program::list(c, school_year_id))
}

