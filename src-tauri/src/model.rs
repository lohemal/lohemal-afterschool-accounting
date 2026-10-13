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

// ── P2: 배분 · 대조 ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Allocation {
    pub id: i64,
    pub entry_id: i64,
    pub program_id: i64,
    pub program_name: String,
    pub amount: i64,
    pub method: String,
    pub status: String,
    pub confirmed: bool,
    pub reason: String,
    pub created_at: String,
    pub source_row_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocCandidate {
    pub program_id: i64,
    pub program_name: String,
    pub item_code: String,
    pub amount: i64,
    pub source_upload_id: i64,
    pub source_row_id: i64,
    /// 학년·반·번호·이름이 모두 같은 학생인가 (false 면 이름만 같은 후보)
    pub same_student: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingEntry {
    pub entry_id: i64,
    pub entry_kind: String,
    pub student: String,
    pub student_id: Option<i64>,
    pub fund_name: String,
    pub item_code: String,
    pub item_name: String,
    pub amount: i64,
    pub allocated: i64,
    pub remaining: i64,
    /// auto · manual · name_only · no_roster · no_student · done
    pub plan: String,
    pub candidates: Vec<AllocCandidate>,
    pub allocations: Vec<Allocation>,
    pub file_name: String,
    pub version_no: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AllocationBoard {
    pub fiscal_month_id: i64,
    pub roster_upload_count: i64,
    pub entries: Vec<PendingEntry>,
    pub unallocated_total: i64,
    pub allocated_total: i64,
    pub auto_possible: i64,
    pub need_user: i64,
    pub no_student: i64,
    pub by_item: Vec<SummaryLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AutoAllocateResult {
    pub entries_allocated: i64,
    pub lines: i64,
    pub skipped: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualPart {
    pub program_id: i64,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FundLine {
    /// 통합을 푼 재원 id. None = 미확정
    pub fund_id: Option<i64>,
    pub fund_name: String,
    /// 통합 보기에서 묶이는 뿌리 재원 id (상위가 있으면 상위)
    pub root_id: Option<i64>,
    pub basis: Option<i64>,
    pub expense: Option<i64>,
    pub diff: Option<i64>,
    /// ok · mismatch · no_basis · no_expense · unmapped
    pub state: String,
}

/// 비교에서 뺀 재원과 그 까닭.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExcludedFund {
    pub fund_name: String,
    pub basis: i64,
    pub expense: i64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconRow {
    pub unit_kind: String,
    pub unit_id: i64,
    pub unit_name: String,
    pub item_set: String,
    pub item_label: String,
    pub initial: i64,
    pub extra: i64,
    pub refund: i64,
    /// 재원별 기준액에 반영된 재원경정 거래 수 (쌍의 합은 0)
    pub transfer_count: i64,
    pub allocated_in: i64,
    /// 전체 징수 기준액 = 비교 가능 + 비교 제외 + 매핑 미확정
    pub basis: i64,
    /// 전체 지급액 = 비교 가능 + 비교 제외 + 매핑 미확정
    pub expense: i64,
    /// 전체 단순 차액 = basis − expense (참고 지표)
    pub simple_diff: i64,
    pub comparable_basis: i64,
    pub comparable_expense: i64,
    /// 재원별 대조 차액 = comparable_basis − comparable_expense (판정 기준)
    pub diff: i64,
    pub excluded_basis: i64,
    pub excluded_expense: i64,
    pub unmapped_basis: i64,
    pub unmapped_expense: i64,
    /// 이 달·이 항목묶음의 미배분 징수액 (프로그램 공통)
    pub unallocated_in_set: i64,
    pub excluded: Vec<ExcludedFund>,
    /// 비교 가능한 재원의 총액은 같은데 재원별로 어긋남 (총액 일치 · 재원 불일치)
    pub total_match_fund_mismatch: bool,
    /// ok · mismatch · missing · unallocated · unmapped · unverifiable · group_only
    pub status: String,
    pub status_label: String,
    pub hints: Vec<String>,
    /// 세부 재원 보기
    pub funds: Vec<FundLine>,
    /// 수익자 통합 보기 (합계는 세부 보기와 같다)
    pub funds_merged: Vec<FundLine>,
    pub note_status: String,
    pub note_label: String,
    pub needs_recheck: bool,
    pub fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthRecon {
    pub fiscal_month_id: i64,
    pub collection_total: i64,
    pub expenditure_total: i64,
    /// 전체 단순 차액 (참고)
    pub simple_diff: i64,
    pub comparable_basis: i64,
    pub comparable_expense: i64,
    pub excluded_basis: i64,
    pub excluded_expense: i64,
    pub unallocated_total: i64,
    pub todo_count: i64,
    pub mismatch_count: i64,
    pub missing_count: i64,
    pub rows: Vec<ReconRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TraceEntry {
    pub entry_id: i64,
    pub entry_kind: String,
    pub kind_label: String,
    pub program_name: String,
    pub party: String,
    pub fund_name: String,
    pub fund_text: String,
    pub item_name: String,
    pub amount: i64,
    /// 이 단위에 귀속된 금액 (배분이면 배분액, 직접이면 amount)
    pub attributed: i64,
    pub via_allocation: bool,
    pub month_label: String,
    pub processed_on: Option<String>,
    pub state: String,
    pub file_id: Option<i64>,
    pub file_name: String,
    pub version_no: i64,
    pub file_status: String,
    pub sheet: String,
    pub row_no: i64,
    pub source_row_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconNote {
    pub id: i64,
    pub status: String,
    pub reason: String,
    pub fingerprint: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReconDetail {
    pub row: ReconRow,
    pub month_label: String,
    pub members: Vec<String>,
    pub collection: Vec<TraceEntry>,
    pub expenditure: Vec<TraceEntry>,
    pub parties: Vec<SummaryLine>,
    pub notes: Vec<ReconNote>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRowView {
    pub file_id: i64,
    pub file_name: String,
    pub version_no: i64,
    pub stored_name: String,
    pub sheet: String,
    pub row_no: i64,
    pub cells: Vec<String>,
}

// ── P3: 수동 회계 정정 ────────────────────────────────────────────────────

/// 정정 등록 입력. kind: fund_transfer · refund · refund_return · extra_collect
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentInput {
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub kind: String,
    pub program_id: i64,
    pub student_id: Option<i64>,
    pub from_fund_id: Option<i64>,
    pub to_fund_id: Option<i64>,
    pub fund_type_id: Option<i64>,
    pub amount_item_id: i64,
    pub amount: i64,
    pub decided_on: Option<String>,
    pub processed_on: Option<String>,
    /// planned · done
    pub status: String,
    pub reason: String,
    pub related_entry_id: Option<i64>,
    pub related_adjustment_id: Option<i64>,
    /// 중복 가능성 경고를 보고도 등록하겠다는 확인
    #[serde(default)]
    pub confirm_duplicate: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FundBefore {
    pub fund_id: i64,
    pub fund_name: String,
    pub before: i64,
    pub change: i64,
    pub after: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentPreview {
    pub kind_label: String,
    pub program_name: String,
    pub student: String,
    pub month_label: String,
    pub month_closed: bool,
    /// 재원별 경정 전·변동·후 (경정이면 두 줄, 환불·추가징수면 한 줄)
    pub funds: Vec<FundBefore>,
    pub total_before: i64,
    pub total_after: i64,
    /// 환불 가능 금액 (수강료 환불일 때)
    pub refundable: Option<i64>,
    pub existing_refunds: i64,
    /// 같은 경제적 사건일 수 있는 기존 거래·정정
    pub duplicates: Vec<String>,
    pub warnings: Vec<String>,
    pub blocked: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Adjustment {
    pub id: i64,
    pub school_year_id: i64,
    pub fiscal_month_id: i64,
    pub month_label: String,
    pub kind: String,
    pub kind_label: String,
    pub scope: String,
    pub program_id: i64,
    pub program_name: String,
    pub student_id: Option<i64>,
    pub student: String,
    pub from_fund_name: String,
    pub to_fund_name: String,
    pub fund_name: String,
    pub item_name: String,
    pub amount: i64,
    pub decided_on: Option<String>,
    pub processed_on: Option<String>,
    pub status: String,
    pub status_label: String,
    pub reason: String,
    pub related_entry_id: Option<i64>,
    pub related_entry_state: String,
    pub related_adjustment_id: Option<i64>,
    pub entry_ids: Vec<i64>,
    /// 원본 거래가 취소되어 근거를 다시 확인해야 함
    pub orphan: bool,
    pub created_at: String,
    pub cancelled_at: Option<String>,
    pub cancel_reason: String,
    pub logs: Vec<AdjustmentLog>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentLog {
    pub from_status: String,
    pub to_status: String,
    pub note: String,
    pub at: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AdjustmentFilter {
    pub school_year_id: i64,
    pub fiscal_month_id: Option<i64>,
    pub processed_from: Option<String>,
    pub processed_to: Option<String>,
    pub program_id: Option<i64>,
    pub student_text: Option<String>,
    pub fund_type_id: Option<i64>,
    pub kind: Option<String>,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StudentRow {
    pub id: i64,
    pub label: String,
}

/// 학생·프로그램·귀속월의 유효 징수 거래 (환불·추가징수 등록 때 원본 거래 선택용)
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryPick {
    pub id: i64,
    pub kind_label: String,
    pub fund_name: String,
    pub item_name: String,
    pub amount: i64,
    pub stage: String,
    pub file_name: String,
}

// ── P4: 자유수강권 ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherPolicy {
    pub id: i64,
    pub school_year_id: i64,
    /// annual · semester · unlimited
    pub basis: String,
    pub basis_label: String,
    pub annual_limit: Option<i64>,
    pub semester1_limit: Option<i64>,
    pub semester2_limit: Option<i64>,
    pub carry_over: bool,
    /// 학기 기간 (저장값이 없으면 기본값: 1학기 3/1~8/31 · 2학기 9/1~다음 해 2월 말)
    pub semester1_from: String,
    pub semester1_to: String,
    pub semester2_from: String,
    pub semester2_to: String,
    pub semester_default: bool,
    /// 어느 학기에도 들지 않는 회계월 (미지원 기간)
    pub uncovered_months: Vec<String>,
    /// 지원 대상 항목 id (비어 있으면 '설정 없음 = 모든 항목')
    pub eligible_item_ids: Vec<i64>,
    pub eligible_item_names: Vec<String>,
    pub all_items_eligible: bool,
    /// 학기 공백 달을 담당자가 확인하고 저장했음
    pub gap_confirmed: bool,
    pub note: String,
    pub created_at: String,
    pub superseded_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct VoucherPolicyInput {
    pub school_year_id: i64,
    pub basis: String,
    pub annual_limit: Option<i64>,
    pub semester1_limit: Option<i64>,
    pub semester2_limit: Option<i64>,
    #[serde(default)]
    pub carry_over: bool,
    /// 비우면 기본값
    pub semester1_from: Option<String>,
    pub semester1_to: Option<String>,
    pub semester2_from: Option<String>,
    pub semester2_to: Option<String>,
    /// None = 모든 항목 지원 (P4 호환). Some(빈 목록) 은 거부.
    pub eligible_item_ids: Option<Vec<i64>>,
    /// 학기 사이 공백 달이 있으면 이 확인 없이는 저장되지 않는다.
    #[serde(default)]
    pub confirm_gap: bool,
    #[serde(default)]
    pub note: String,
}

/// 정책 저장 전 영향 미리보기.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherPolicyPreview {
    pub semester1_from: String,
    pub semester1_to: String,
    pub semester2_from: String,
    pub semester2_to: String,
    pub uncovered_months: Vec<String>,
    /// 공백 달이 있어 담당자 확인이 필요한가
    pub gap_needs_confirm: bool,
    pub eligible_item_names: Vec<String>,
    /// 지원 대상에서 빠지는 항목에 이미 등록된(살아 있는) 자유수강권 경정
    pub affected_transfer_count: i64,
    pub affected_transfer_total: i64,
    pub warnings: Vec<String>,
    pub blocked: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherException {
    pub id: i64,
    pub school_year_id: i64,
    pub student_id: i64,
    pub student: String,
    pub basis: String,
    pub basis_label: String,
    pub annual_limit: Option<i64>,
    pub semester1_limit: Option<i64>,
    pub semester2_limit: Option<i64>,
    pub carry_over: bool,
    pub reason: String,
    pub created_at: String,
    pub superseded_at: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherExceptionInput {
    pub school_year_id: i64,
    pub student_id: i64,
    pub basis: String,
    pub annual_limit: Option<i64>,
    pub semester1_limit: Option<i64>,
    pub semester2_limit: Option<i64>,
    #[serde(default)]
    pub carry_over: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherTargetLog {
    pub action: String,
    pub from_status: String,
    pub to_status: String,
    pub detail: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherTarget {
    pub id: i64,
    pub school_year_id: i64,
    pub student_id: i64,
    pub student: String,
    /// pending · confirmed · ended · cancelled
    pub status: String,
    pub status_label: String,
    pub confirmed_on: Option<String>,
    pub apply_from: Option<String>,
    pub apply_to: Option<String>,
    pub transfer_in: bool,
    pub transfer_in_on: Option<String>,
    pub prior_used: Option<i64>,
    /// unconfirmed · confirmed
    pub prior_status: String,
    pub prior_status_label: String,
    pub prior_note: String,
    /// 전입생인데 이전 학교 사용액이 확인되지 않음 → 사용액 계산 차단
    pub prior_needs_confirm: bool,
    pub has_exception: bool,
    pub note: String,
    pub created_at: String,
    pub updated_at: String,
    pub logs: Vec<VoucherTargetLog>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherTargetInput {
    pub school_year_id: i64,
    pub student_id: i64,
    pub status: String,
    pub confirmed_on: Option<String>,
    pub apply_from: Option<String>,
    pub apply_to: Option<String>,
    #[serde(default)]
    pub transfer_in: bool,
    pub transfer_in_on: Option<String>,
    #[serde(default)]
    pub note: String,
    /// 상태를 바꾸는 사유 (이력에 남는다)
    #[serde(default)]
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherPriorInput {
    pub target_id: i64,
    pub prior_used: Option<i64>,
    /// true 면 '확인됨'. 0원도 담당자가 명시해야 확인된다.
    #[serde(default)]
    pub confirmed: bool,
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StudentCreateInput {
    pub school_year_id: i64,
    pub grade: i32,
    pub class_name: String,
    pub student_no: String,
    pub name: String,
}

/// 담당자가 정한 한 달 안 프로그램별 자유수강권 배분.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherDecision {
    pub student_id: i64,
    pub fiscal_month_id: i64,
    pub program_id: i64,
    pub amount_item_id: i64,
    pub free_amount: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherFundPart {
    pub fund_id: i64,
    pub fund_name: String,
    pub amount: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherUsageRow {
    pub fiscal_month_id: i64,
    pub ym: String,
    pub month_label: String,
    pub seq: i32,
    pub program_id: i64,
    pub program_name: String,
    pub amount_item_id: i64,
    pub item_name: String,
    /// 수강료(징수 기준액 합) = 자유수강권 + 수익자 + 기타
    pub tuition: i64,
    pub free_actual: i64,
    /// 수익자 계열 합 = 초3 초과금 + 일반 수익자
    pub beneficiary_actual: i64,
    pub g3x_actual: i64,
    pub general_actual: i64,
    /// 방과후이용권 (자유수강권 사용액에 넣지 않고 전환 대상도 아님)
    pub voucher_actual: i64,
    pub other_actual: i64,
    pub funds: Vec<VoucherFundPart>,
    /// 지원 대상 항목인가 (아니면 계산에서 제외, 기존 재원 유지)
    pub eligible_item: bool,
    /// 1 · 2 · 0(어느 학기에도 속하지 않음)
    pub semester: i32,
    /// 계산 지원액 (적용 기간 밖이면 0)
    pub free_expected: i64,
    pub beneficiary_expected: i64,
    /// 처리 예정 재원경정(→자유수강권) 금액
    pub pending_transfer: i64,
    /// free_expected − free_actual − pending_transfer. 양수면 경정(소급) 대상, 음수면 계산보다 많이 쓴 것
    pub delta: i64,
    pub remaining_before: Option<i64>,
    pub remaining_after: Option<i64>,
    /// ok · retro · over · decide · pending · outside
    pub state: String,
    pub state_label: String,
    pub in_window: bool,
    pub month_closed: bool,
    /// 이 칸에서 전환 가능한 초3 초과금 · 일반 수익자 (delta 를 우선순위로 나눈 것)
    pub retro_g3x: i64,
    pub retro_general: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherUsage {
    pub student_id: i64,
    pub student: String,
    pub target_id: Option<i64>,
    pub target_status: String,
    pub target_status_label: String,
    pub apply_from: Option<String>,
    pub apply_to: Option<String>,
    pub basis: String,
    pub basis_label: String,
    /// 정책 · 학생 예외
    pub limit_source: String,
    pub limit_label: String,
    pub limit_total: Option<i64>,
    pub prior_used: Option<i64>,
    pub used_expected: i64,
    pub used_actual: i64,
    /// 방과후이용권 사용액 (별도 집계, 자유수강권에 포함하지 않음)
    pub voucher_total: i64,
    pub g3x_total: i64,
    pub general_total: i64,
    /// 방과후이용권과 자유수강권 중복 대상자 (이용권 대상자로 확인되었거나 이용권 실적이 있음)
    pub dual: bool,
    /// 방과후이용권 대상 여부 (none · unconfirmed · eligible · ineligible · ended)
    pub voucher_target_status: String,
    pub voucher_target_status_label: String,
    pub eligible_item_names: Vec<String>,
    pub semester_label: String,
    pub pending_total: i64,
    pub remaining: Option<i64>,
    pub remaining_s1: Option<i64>,
    pub remaining_s2: Option<i64>,
    pub exhausted_ym: Option<String>,
    pub retro_total: i64,
    pub over_total: i64,
    pub needs_decision: bool,
    pub decision_months: Vec<String>,
    pub rows: Vec<VoucherUsageRow>,
    pub blocked: Vec<String>,
    pub warnings: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherProposal {
    pub key: String,
    pub student_id: i64,
    pub student: String,
    pub fiscal_month_id: i64,
    pub month_label: String,
    pub program_id: i64,
    pub program_name: String,
    pub amount_item_id: i64,
    pub item_name: String,
    pub from_fund_id: Option<i64>,
    pub from_fund_name: String,
    pub amount: i64,
    /// normal · check(재원 확인 필요 — 담당자 확인 뒤 등록 가능) · blocked · decide · pending · over · short
    pub state: String,
    /// 이 칸의 기존 재원 구성 (이용권 · 초3 초과금 · 일반 수익자 · 자유수강권)
    pub voucher_actual: i64,
    pub g3x_actual: i64,
    pub general_actual: i64,
    pub free_actual: i64,
    /// 전환 뒤 그 재원에 남는 수익자 부담금
    pub remaining_after: i64,
    pub limit_after: Option<i64>,
    pub state_label: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherRetroPreview {
    pub usages: Vec<VoucherUsage>,
    pub proposals: Vec<VoucherProposal>,
    pub normal_count: i64,
    pub normal_total: i64,
    pub check_count: i64,
    pub check_total: i64,
    pub blocked_count: i64,
    pub student_count: i64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherApplyRow {
    pub student_id: i64,
    pub fiscal_month_id: i64,
    pub program_id: i64,
    pub amount_item_id: i64,
    pub from_fund_id: i64,
    pub amount: i64,
    /// '재원 확인 필요' 행을 담당자가 확인했다는 표시
    #[serde(default)]
    pub confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherApplyInput {
    pub school_year_id: i64,
    /// retro · monthly
    pub kind: String,
    pub reason: String,
    pub decided_on: Option<String>,
    pub processed_on: Option<String>,
    /// planned · done
    pub status: String,
    pub rows: Vec<VoucherApplyRow>,
    #[serde(default)]
    pub decisions: Vec<VoucherDecision>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherBatchItem {
    pub adjustment_id: i64,
    pub student_id: i64,
    pub student: String,
    pub month_label: String,
    pub program_name: String,
    pub item_name: String,
    pub from_fund_name: String,
    pub amount: i64,
    pub status: String,
    pub status_label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherBatch {
    pub id: i64,
    pub school_year_id: i64,
    pub kind: String,
    pub kind_label: String,
    pub student_id: Option<i64>,
    pub student: String,
    pub reason: String,
    pub item_count: i64,
    pub total_amount: i64,
    pub created_at: String,
    pub items: Vec<VoucherBatchItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherRefundReturn {
    pub adjustment_id: i64,
    pub amount: i64,
    pub status: String,
    pub status_label: String,
    pub processed_on: Option<String>,
    pub collected_confirmed: bool,
    pub evidence: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherRefundRow {
    pub transfer_id: i64,
    pub student_id: i64,
    pub student: String,
    pub fiscal_month_id: i64,
    pub month_label: String,
    pub program_name: String,
    pub item_name: String,
    pub from_fund_id: i64,
    pub from_fund_name: String,
    pub amount: i64,
    pub transfer_status: String,
    pub transfer_status_label: String,
    pub transfer_processed_on: Option<String>,
    pub from_batch: bool,
    pub batch_id: Option<i64>,
    pub returned_done: i64,
    pub returned_planned: i64,
    pub outstanding: i64,
    /// none · planned · partial · complete · cancelled
    pub status: String,
    pub status_label: String,
    pub orphan: bool,
    pub returns: Vec<VoucherRefundReturn>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherRefundInput {
    pub transfer_id: i64,
    pub amount: i64,
    /// planned · done
    pub status: String,
    pub processed_on: Option<String>,
    /// 실제 수납(학부모가 실제로 낸 돈) 근거를 확인했는가 — 처리 완료에 필수
    #[serde(default)]
    pub collected_confirmed: bool,
    #[serde(default)]
    pub evidence: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherTargetLogRow {
    pub target_id: i64,
    pub student: String,
    pub action: String,
    pub from_status: String,
    pub to_status: String,
    pub detail: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherHistory {
    pub batches: Vec<VoucherBatch>,
    pub target_logs: Vec<VoucherTargetLogRow>,
    pub policy_history: Vec<VoucherPolicy>,
    pub exception_history: Vec<VoucherException>,
}

// ── P5: 운영 — 백업 · 복원 · 무결성 · 내보내기 · 이용권 대상 ─────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpsRecord {
    pub at: String,
    /// backup · restore · integrity · export · cleanup
    pub kind: String,
    /// ok · failed · blocked
    pub status: String,
    pub summary: String,
    pub file: Option<String>,
    #[serde(default)]
    pub detail: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
}

/// 백업 패키지 안의 manifest.json
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupManifest {
    pub format_version: u32,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i32,
    /// manual · auto · migration · pre_restore
    pub kind: String,
    pub note: String,
    pub years: Vec<String>,
    pub db_file: String,
    pub files: Vec<ManifestFile>,
    /// 백업에 넣지 못한 원본 파일 (보관 폴더에 없음)
    pub missing_files: Vec<String>,
    pub complete: bool,
    /// 모든 파일의 "path:sha256" 줄을 이어 붙인 것의 SHA-256
    pub package_sha256: String,
    pub counts: DbCounts,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DbCounts {
    pub schema_version: i32,
    pub years: Vec<String>,
    pub ledger_entries: i64,
    pub active_entries: i64,
    pub adjustments: i64,
    pub refunds: i64,
    pub voucher_targets: i64,
    pub voucher_batches: i64,
    pub uploads: i64,
    pub students: i64,
    pub audit_logs: i64,
    pub month_closes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupVerify {
    pub ok: bool,
    pub checked_at: String,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub file_name: String,
    pub path: String,
    pub size: u64,
    pub kind: String,
    pub kind_label: String,
    pub created_at: String,
    pub app_version: String,
    pub schema_version: i32,
    pub years: Vec<String>,
    pub complete: bool,
    pub file_count: i64,
    pub verified: bool,
    pub verify_errors: Vec<String>,
    pub counts: Option<DbCounts>,
    /// 자동 정리 대상에서 제외되는가 (수동·마이그레이션·복원 전)
    pub protected: bool,
    /// 이전 형식(.db 단일 파일) — 목록에만 보이고 복원은 안 됨
    pub legacy: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupList {
    pub dir: String,
    pub backups: Vec<BackupInfo>,
    pub failed: Vec<BackupInfo>,
    pub last_backup_at: Option<String>,
    pub auto_enabled: bool,
    pub keep: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestorePreview {
    pub path: String,
    pub manifest: Option<BackupManifest>,
    pub verify: BackupVerify,
    pub current: DbCounts,
    pub backup: Option<DbCounts>,
    pub blocked: Vec<String>,
    pub warnings: Vec<String>,
    pub can_restore: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub ok: bool,
    pub message: String,
    pub safety_backup: Option<String>,
    pub before: DbCounts,
    pub after: DbCounts,
    pub before_db_sha256: String,
    pub after_db_sha256: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityItem {
    pub code: String,
    /// ok · check · error
    pub level: String,
    pub title: String,
    pub detail: String,
    pub student: String,
    pub program: String,
    pub month: String,
    pub entry_ids: Vec<i64>,
    pub cause: String,
    pub how_to_check: String,
    pub fix_hint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityCheck {
    pub code: String,
    pub title: String,
    pub level: String,
    pub count: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IntegrityReport {
    pub checked_at: String,
    pub school_year_id: i64,
    pub ok_count: i64,
    pub check_count: i64,
    pub error_count: i64,
    pub checks: Vec<IntegrityCheck>,
    pub items: Vec<IntegrityItem>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    pub path: String,
    pub sheet: String,
    pub rows: i64,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ReconExportFilter {
    pub status: Option<String>,
    pub item_set: Option<String>,
    pub program_text: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct RefundExportFilter {
    pub status: Option<String>,
    pub fund_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AfterVoucherLog {
    pub action: String,
    pub from_status: String,
    pub to_status: String,
    pub detail: String,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AfterVoucherTarget {
    pub id: i64,
    pub school_year_id: i64,
    pub student_id: i64,
    pub student: String,
    /// unconfirmed · eligible · ineligible · ended
    pub status: String,
    pub status_label: String,
    pub apply_from: Option<String>,
    pub apply_to: Option<String>,
    pub confirmed_on: Option<String>,
    pub evidence: String,
    pub note: String,
    /// 원장의 방과후이용권 실적 (학년도 합)
    pub actual_total: i64,
    pub created_at: String,
    pub updated_at: String,
    pub logs: Vec<AfterVoucherLog>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AfterVoucherTargetInput {
    pub school_year_id: i64,
    pub student_id: i64,
    pub status: String,
    pub apply_from: Option<String>,
    pub apply_to: Option<String>,
    pub confirmed_on: Option<String>,
    #[serde(default)]
    pub evidence: String,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub reason: String,
}
