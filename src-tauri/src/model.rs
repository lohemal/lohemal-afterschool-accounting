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
    /// 다른 재원에 통합된 경우 그 재원 (예: 초3 지원금 → 방과후이용권)
    pub merged_into_id: Option<i64>,
    pub sort_order: i32,
    pub is_builtin: bool,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundAlias {
    pub id: i64,
    pub fund_type_id: i64,
    pub fund_type_name: String,
    pub alias: String,
    pub is_builtin: bool,
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
    pub entry_count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramGroup {
    pub id: i64,
    pub school_year_id: i64,
    pub name: String,
    pub note: String,
    pub member_ids: Vec<i64>,
    pub member_names: Vec<String>,
}

/// 프로그램 표기 매핑 저장 입력. target: program · group · new_program · new_group
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgramMapInput {
    pub school_year_id: i64,
    pub alias: String,
    pub target: String,
    pub program_id: Option<i64>,
    pub group_id: Option<i64>,
    pub new_name: Option<String>,
    pub member_ids: Option<Vec<i64>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UploadSummary {
    pub source_row_count: i64,
    pub data_row_count: i64,
    pub excluded_row_count: i64,
    pub row_count: i64,
    pub ok_count: i64,
    pub check_count: i64,
    pub error_count: i64,
    pub included_count: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
    pub by_item: Vec<SummaryLine>,
    pub by_fund: Vec<SummaryLine>,
    pub by_program: Vec<SummaryLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SummaryLine {
    pub name: String,
    pub count: i64,
    pub total: i64,
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
    pub format_label: String,
    pub data_kind: String,
    pub data_kind_label: String,
    pub fiscal_month_id: Option<i64>,
    pub fiscal_month_label: String,
    pub lineage_id: Option<i64>,
    pub version_no: i32,
    pub status: String,
    pub status_label: String,
    pub replaced_by_id: Option<i64>,
    pub stored_name: String,
    pub note: String,
    pub uploaded_at: String,
    pub applied_at: Option<String>,
    pub cancelled_at: Option<String>,
    pub row_count: i64,
    /// 살아 있는(active) 거래 수
    pub entry_count: i64,
    pub active_collection_total: i64,
    pub active_expenditure_total: i64,
    pub summary: UploadSummary,
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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SheetInfo {
    pub name: String,
    pub max_row: u32,
    pub max_col: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inspect {
    pub path: String,
    pub file_name: String,
    pub byte_size: i64,
    pub sha256: String,
    pub sheets: Vec<SheetInfo>,
    pub detected: Vec<crate::excel::detect::Detected>,
    pub suggested_month: Option<u32>,
    /// 같은 내용의 파일이 이미 살아 있으면 그 파일
    pub duplicate_of: Option<UploadFile>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RowOverride {
    pub sheet: String,
    pub row_no: u32,
    pub program_id: Option<i64>,
    pub program_group_id: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRequest {
    pub path: String,
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub format_code: String,
    pub data_kind: String,
    #[serde(default)]
    pub overrides: Vec<RowOverride>,
    /// 이 파일을 대체(교체)할 기존 파일
    pub replace_file_id: Option<i64>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreviewRow {
    pub seq: i64,
    pub sheet: String,
    pub row_no: u32,
    pub program_text: String,
    pub program_name: String,
    pub program_suggestion: String,
    pub student: String,
    pub fund_text: String,
    pub fund_name: String,
    pub item_name: String,
    pub entry_kind: String,
    pub amount: i64,
    pub counterparty: String,
    pub status: String,
    pub included: bool,
    pub issues: Vec<crate::excel::parse::Issue>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedProgram {
    pub text: String,
    pub count: i64,
    pub suggestion_program_id: Option<i64>,
    pub suggestion_name: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnresolvedFund {
    pub text: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub format_code: String,
    pub format_label: String,
    pub data_kind: String,
    pub data_kind_label: String,
    pub summary: UploadSummary,
    pub rows: Vec<PreviewRow>,
    pub unresolved_programs: Vec<UnresolvedProgram>,
    pub unresolved_funds: Vec<UnresolvedFund>,
    pub file_issues: Vec<crate::excel::parse::Issue>,
    pub can_commit: bool,
    pub block_reasons: Vec<String>,
    pub warnings: Vec<String>,
    /// 같은 귀속월·같은 자료 유형으로 이미 적용 중인 파일 (교체 대상 후보)
    pub existing_applied: Option<UploadFile>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommitResult {
    pub file: UploadFile,
    pub entry_count: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub entry_count: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
    pub program_count: i64,
    pub by_fund: Vec<SummaryLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Dependent {
    pub file_id: i64,
    pub file_name: String,
    pub data_kind_label: String,
    pub entry_count: i64,
}

/// 취소·복원 전 미리보기.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Impact {
    pub action: String,
    pub file: UploadFile,
    /// 복원 때 현재 적용 중인 판 (취소될 판)
    pub current: Option<UploadFile>,
    pub before: Totals,
    pub after: Totals,
    pub month_label: String,
    pub dependents: Vec<Dependent>,
    pub blocked: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadApply {
    pub id: i64,
    pub action: String,
    pub entry_count: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
    pub note: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadDetail {
    pub file: UploadFile,
    pub versions: Vec<UploadFile>,
    pub rows: Vec<PreviewRow>,
    pub applies: Vec<UploadApply>,
    pub source_rows: Vec<crate::excel::parse::SourceRow>,
}

/// 새 회계 거래. 금액은 원 단위 정수(부호 있음).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewEntry {
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub program_id: Option<i64>,
    #[serde(default)]
    pub program_group_id: Option<i64>,
    pub student_id: Option<i64>,
    pub fund_type_id: Option<i64>,
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
    pub program_id: Option<i64>,
    pub program_group_id: Option<i64>,
    pub student_id: Option<i64>,
    pub fund_type_id: Option<i64>,
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
    pub program_id: Option<i64>,
    pub program_name: String,
    pub fund_type_id: Option<i64>,
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
