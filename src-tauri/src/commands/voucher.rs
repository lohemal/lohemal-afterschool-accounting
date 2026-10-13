use tauri::State;

use crate::db::Db;
use crate::error::AppResult;
use crate::model::*;
use crate::repo::voucher;

#[tauri::command]
pub fn voucher_policy_current(db: State<Db>, school_year_id: i64) -> AppResult<Option<VoucherPolicy>> {
    db.read(|c| voucher::policy_current(c, school_year_id))
}

#[tauri::command]
pub fn voucher_policy_history(db: State<Db>, school_year_id: i64) -> AppResult<Vec<VoucherPolicy>> {
    db.read(|c| voucher::policy_history(c, school_year_id))
}

#[tauri::command]
pub fn voucher_policy_save(db: State<Db>, input: VoucherPolicyInput) -> AppResult<VoucherPolicy> {
    db.write(|tx| voucher::policy_save(tx, &input))
}

#[tauri::command]
pub fn voucher_exception_list(db: State<Db>, school_year_id: i64, include_history: Option<bool>) -> AppResult<Vec<VoucherException>> {
    db.read(|c| voucher::exception_list(c, school_year_id, include_history.unwrap_or(false)))
}

#[tauri::command]
pub fn voucher_exception_save(db: State<Db>, input: VoucherExceptionInput) -> AppResult<VoucherException> {
    db.write(|tx| voucher::exception_save(tx, &input))
}

#[tauri::command]
pub fn voucher_exception_end(db: State<Db>, id: i64, reason: String) -> AppResult<()> {
    db.write(|tx| voucher::exception_end(tx, id, &reason))
}

#[tauri::command]
pub fn voucher_target_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<VoucherTarget>> {
    db.read(|c| voucher::target_list(c, school_year_id))
}

#[tauri::command]
pub fn voucher_target_save(db: State<Db>, input: VoucherTargetInput) -> AppResult<VoucherTarget> {
    db.write(|tx| voucher::target_save(tx, &input))
}

#[tauri::command]
pub fn voucher_prior_set(db: State<Db>, input: VoucherPriorInput) -> AppResult<VoucherTarget> {
    db.write(|tx| voucher::target_prior_set(tx, &input))
}

#[tauri::command]
pub fn student_create(db: State<Db>, input: StudentCreateInput) -> AppResult<StudentRow> {
    db.write(|tx| voucher::student_create(tx, &input))
}

#[tauri::command]
pub fn voucher_usage(db: State<Db>, school_year_id: i64, student_id: i64, decisions: Option<Vec<VoucherDecision>>) -> AppResult<VoucherUsage> {
    db.read(|c| voucher::usage(c, school_year_id, student_id, &decisions.unwrap_or_default()))
}

#[tauri::command]
pub fn voucher_usage_all(db: State<Db>, school_year_id: i64) -> AppResult<Vec<VoucherUsage>> {
    db.read(|c| voucher::usage_all(c, school_year_id))
}

#[tauri::command]
pub fn voucher_retro_preview(db: State<Db>, school_year_id: i64, student_ids: Option<Vec<i64>>, decisions: Option<Vec<VoucherDecision>>) -> AppResult<VoucherRetroPreview> {
    db.read(|c| voucher::retro_preview(c, school_year_id, &student_ids.unwrap_or_default(), &decisions.unwrap_or_default()))
}

#[tauri::command]
pub fn voucher_apply(db: State<Db>, input: VoucherApplyInput) -> AppResult<VoucherBatch> {
    db.write(|tx| voucher::apply(tx, &input))
}

#[tauri::command]
pub fn voucher_batch_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<VoucherBatch>> {
    db.read(|c| voucher::batch_list(c, school_year_id))
}

#[tauri::command]
pub fn voucher_refund_list(db: State<Db>, school_year_id: i64) -> AppResult<Vec<VoucherRefundRow>> {
    db.read(|c| voucher::refund_list(c, school_year_id))
}

#[tauri::command]
pub fn voucher_refund_register(db: State<Db>, input: VoucherRefundInput) -> AppResult<Adjustment> {
    db.write(|tx| voucher::refund_register(tx, &input))
}

#[tauri::command]
pub fn voucher_refund_complete(db: State<Db>, adjustment_id: i64, processed_on: String, collected_confirmed: bool, evidence: String) -> AppResult<Adjustment> {
    db.write(|tx| voucher::refund_complete(tx, adjustment_id, &processed_on, collected_confirmed, &evidence))
}

#[tauri::command]
pub fn voucher_history(db: State<Db>, school_year_id: i64) -> AppResult<VoucherHistory> {
    db.read(|c| voucher::history(c, school_year_id))
}
