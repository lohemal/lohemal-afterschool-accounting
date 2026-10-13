//! 화면과 주고받는 자료형. `src/ipc/types.ts` 와 짝을 이룬다 — 한쪽을 고치면 다른 쪽도 고친다.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub db_path: String,
    pub data_dir: String,
    pub schema_version: i32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchoolYear {
    pub id: i64,
    pub year: i32,
    pub name: String,
    pub start_ym: String,
    pub end_ym: String,
    pub is_current: bool,
    pub created_at: String,
}

/// 회계월 한 칸. 합계는 저장된 값이 아니라 `ledger_entry` 에서 그때그때 계산한 값이다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FiscalMonth {
    pub id: i64,
    pub school_year_id: i64,
    pub ym: String,
    pub seq: i32,
    pub label: String,
    pub status: String,
    pub entry_count: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
    pub last_closed_at: Option<String>,
    /// 마지막 확정 이후 새 거래가 들어왔는가 (소급 반영 표시용).
    pub changed_after_close: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundType {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub parent_id: Option<i64>,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmountItem {
    pub id: i64,
    pub code: String,
    pub name: String,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogInput {
    pub id: Option<i64>,
    pub name: String,
    pub parent_id: Option<i64>,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Catalog {
    pub fund_types: Vec<FundType>,
    pub amount_items: Vec<AmountItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Program {
    pub id: i64,
    pub school_year_id: i64,
    pub name: String,
    pub note: String,
    pub is_active: bool,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadFile {
    pub id: i64,
    pub school_year_id: i64,
    pub file_name: String,
    pub sha256: String,
    pub byte_size: i64,
    pub format_code: String,
    pub version_no: i32,
    pub status: String,
    pub replaced_by_id: Option<i64>,
    pub note: String,
    pub uploaded_at: String,
    pub applied_at: Option<String>,
    pub cancelled_at: Option<String>,
    pub row_count: i64,
    pub entry_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewUploadFile {
    pub school_year_id: i64,
    pub file_name: String,
    pub sha256: String,
    pub byte_size: i64,
    pub format_code: String,
    pub note: Option<String>,
}

/// 새 회계 거래. 금액은 원 단위 정수(부호 있음).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntry {
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub program_id: i64,
    pub student_id: Option<i64>,
    pub fund_type_id: i64,
    pub amount_item_id: i64,
    pub flow: String,
    pub entry_kind: String,
    pub amount: i64,
    pub stage: Option<String>,
    pub processed_on: Option<String>,
    pub counterparty: Option<String>,
    pub upload_file_id: Option<i64>,
    pub source_row_id: Option<i64>,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LedgerEntry {
    pub id: i64,
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub program_id: i64,
    pub student_id: Option<i64>,
    pub fund_type_id: i64,
    pub amount_item_id: i64,
    pub flow: String,
    pub entry_kind: String,
    pub amount: i64,
    pub stage: String,
    pub state: String,
    pub processed_on: Option<String>,
    pub counterparty: String,
    pub transfer_group_id: Option<i64>,
    pub upload_file_id: Option<i64>,
    pub source_row_id: Option<i64>,
    pub supersedes_id: Option<i64>,
    pub reason: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Correction {
    pub id: i64,
    pub entry_id: i64,
    pub replacement_entry_id: Option<i64>,
    pub action: String,
    pub category: String,
    pub reason: String,
    pub completion: String,
    pub completed_on: Option<String>,
    pub created_at: String,
}

/// 월별 합계 한 줄 — 프로그램 × 재원 × 항목 × 흐름.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MonthSummaryRow {
    pub program_id: i64,
    pub program_name: String,
    pub fund_type_id: i64,
    pub fund_type_name: String,
    pub amount_item_id: i64,
    pub amount_item_name: String,
    pub flow: String,
    pub stage: String,
    pub entry_count: i64,
    pub total: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthSummary {
    pub fiscal_month_id: i64,
    pub rows: Vec<MonthSummaryRow>,
    pub collection_total: i64,
    pub expenditure_total: i64,
    pub transfer_balance: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub id: i64,
    pub at: String,
    pub kind: String,
    pub target_table: String,
    pub target_id: Option<i64>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub programs: i64,
    pub uploads: i64,
    pub entries: i64,
    pub students: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Bootstrap {
    pub app: AppInfo,
    pub years: Vec<SchoolYear>,
    pub current_year: Option<SchoolYear>,
    pub school_name: String,
    pub counts: Counts,
}
