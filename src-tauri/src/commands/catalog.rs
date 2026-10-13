use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::{AmountItem, Catalog, CatalogInput, FundType};
use crate::repo::catalog;

#[tauri::command]
pub fn catalog_list(db: State<Db>) -> AppResult<Catalog> {
    db.read(catalog::all)
}

#[tauri::command]
pub fn fund_type_save(db: State<Db>, input: CatalogInput) -> AppResult<FundType> {
    db.write(|tx| catalog::fund_type_save(tx, &input))
}

#[tauri::command]
pub fn amount_item_save(db: State<Db>, input: CatalogInput) -> AppResult<AmountItem> {
    db.write(|tx| catalog::amount_item_save(tx, &input))
}
