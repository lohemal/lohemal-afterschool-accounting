use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::{FiscalMonth, SchoolYear};
use crate::repo::year;

#[tauri::command]
pub fn year_list(db: State<Db>) -> AppResult<Vec<SchoolYear>> {
    db.read(year::list)
}

#[tauri::command]
pub fn year_create(db: State<Db>, year: i32) -> AppResult<SchoolYear> {
    db.write(|tx| year::create(tx, year))
}

#[tauri::command]
pub fn year_set_current(db: State<Db>, id: i64) -> AppResult<SchoolYear> {
    db.write(|tx| year::set_current(tx, id))
}

#[tauri::command]
pub fn month_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<FiscalMonth>> {
    db.read(|c| year::months(c, school_year_id))
}
