/** Rust `model.rs` 와 짝을 이루는 타입. 한쪽을 고치면 다른 쪽도 고친다. */

export interface AppInfo {
  version: string
  dbPath: string
  dataDir: string
  schemaVersion: number
}

export interface SchoolYear {
  id: number
  year: number
  name: string
  startYm: string
  endYm: string
  isCurrent: boolean
  createdAt: string
}

export interface FiscalMonth {
  id: number
  schoolYearId: number
  ym: string
  seq: number
  label: string
  status: 'open' | 'closed'
  entryCount: number
  collectionTotal: number
  expenditureTotal: number
  lastClosedAt: string | null
  changedAfterClose: boolean
}

export interface FundType {
  id: number
  code: string
  name: string
  parentId: number | null
  mergedIntoId: number | null
  sortOrder: number
  isBuiltin: boolean
  isActive: boolean
}

export interface FundAlias {
  id: number
  fundTypeId: number
  fundTypeName: string
  alias: string
  isBuiltin: boolean
}

export interface AmountItem {
  id: number
  code: string
  name: string
  sortOrder: number
  isBuiltin: boolean
  isActive: boolean
}

export interface CatalogInput {
  id?: number
  name: string
  parentId?: number | null
  isActive?: boolean
}

export interface Catalog {
  fundTypes: FundType[]
  amountItems: AmountItem[]
}

export interface Program {
  id: number
  schoolYearId: number
  name: string
  note: string
  isActive: boolean
  aliases: string[]
  entryCount: number
}

export interface ProgramGroup {
  id: number
  schoolYearId: number
  name: string
  note: string
  memberIds: number[]
  memberNames: string[]
}

export interface ProgramMapInput {
  schoolYearId: number
  alias: string
  target: 'program' | 'group' | 'new_program' | 'new_group'
  programId?: number | null
  groupId?: number | null
  newName?: string | null
  memberIds?: number[] | null
}

export type UploadStatus = 'registered' | 'previewed' | 'applied' | 'cancelled' | 'replaced'
export type DataKind = 'initial_collect' | 'extra_refund' | 'instructor_pay' | 'vendor_pay' | 'roster_link' | 'snapshot'

export interface SummaryLine {
  name: string
  count: number
  total: number
}

export interface UploadSummary {
  sourceRowCount: number
  dataRowCount: number
  excludedRowCount: number
  rowCount: number
  okCount: number
  checkCount: number
  errorCount: number
  includedCount: number
  collectionTotal: number
  expenditureTotal: number
  byItem: SummaryLine[]
  byFund: SummaryLine[]
  byProgram: SummaryLine[]
}

export interface UploadFile {
  id: number
  schoolYearId: number
  fileName: string
  sha256: string
  byteSize: number
  formatCode: string
  formatLabel: string
  dataKind: DataKind
  dataKindLabel: string
  fiscalMonthId: number | null
  fiscalMonthLabel: string
  lineageId: number | null
  versionNo: number
  status: UploadStatus
  statusLabel: string
  replacedById: number | null
  storedName: string
  note: string
  uploadedAt: string
  appliedAt: string | null
  cancelledAt: string | null
  rowCount: number
  entryCount: number
  activeCollectionTotal: number
  activeExpenditureTotal: number
  summary: UploadSummary
}

export interface Detected {
  format: string
  code: string
  label: string
  sheet: string
  headerRow: number
  confidence: number
}

export interface SheetInfo {
  name: string
  maxRow: number
  maxCol: number
}

export interface Inspect {
  path: string
  fileName: string
  byteSize: number
  sha256: string
  sheets: SheetInfo[]
  detected: Detected[]
  suggestedMonth: number | null
  duplicateOf: UploadFile | null
}

export interface RowOverride {
  sheet: string
  rowNo: number
  programId?: number | null
  programGroupId?: number | null
}

export interface PreviewRequest {
  path: string
  schoolYearId: number
  fiscalMonthId: number
  formatCode: string
  dataKind: DataKind
  overrides: RowOverride[]
  replaceFileId?: number | null
  note?: string | null
}

export interface Issue {
  level: 'info' | 'check' | 'error'
  message: string
}

export interface PreviewRow {
  seq: number
  sheet: string
  rowNo: number
  programText: string
  programName: string
  programSuggestion: string
  student: string
  fundText: string
  fundName: string
  itemName: string
  entryKind: string
  amount: number
  counterparty: string
  status: 'ok' | 'check' | 'error' | 'skip'
  included: boolean
  issues: Issue[]
}

export interface UnresolvedProgram {
  text: string
  count: number
  suggestionProgramId: number | null
  suggestionName: string
}

export interface UnresolvedFund {
  text: string
  count: number
}

export interface Preview {
  formatCode: string
  formatLabel: string
  dataKind: DataKind
  dataKindLabel: string
  summary: UploadSummary
  rows: PreviewRow[]
  unresolvedPrograms: UnresolvedProgram[]
  unresolvedFunds: UnresolvedFund[]
  fileIssues: Issue[]
  canCommit: boolean
  blockReasons: string[]
  warnings: string[]
  existingApplied: UploadFile | null
}

export interface CommitResult {
  file: UploadFile
  entryCount: number
  collectionTotal: number
  expenditureTotal: number
}

export interface Totals {
  entryCount: number
  collectionTotal: number
  expenditureTotal: number
  programCount: number
  byFund: SummaryLine[]
}

export interface Dependent {
  fileId: number
  fileName: string
  dataKindLabel: string
  entryCount: number
}

export interface Impact {
  action: 'cancel' | 'restore'
  file: UploadFile
  current: UploadFile | null
  before: Totals
  after: Totals
  monthLabel: string
  dependents: Dependent[]
  blocked: string[]
  warnings: string[]
}

export interface UploadApply {
  id: number
  action: 'apply' | 'restore' | 'cancel' | 'replace'
  entryCount: number
  collectionTotal: number
  expenditureTotal: number
  note: string
  createdAt: string
}

export interface SourceRow {
  sheet: string
  rowNo: number
  cells: string[]
}

export interface UploadDetail {
  file: UploadFile
  versions: UploadFile[]
  rows: PreviewRow[]
  applies: UploadApply[]
  sourceRows: SourceRow[]
}

export type Flow = 'collection' | 'expenditure' | 'transfer'
export type Stage = 'planned' | 'actual'

export interface MonthSummaryRow {
  programId: number | null
  programName: string
  fundTypeId: number | null
  fundTypeName: string
  amountItemId: number
  amountItemName: string
  flow: Flow
  stage: Stage
  entryCount: number
  total: number
}

export interface MonthSummary {
  fiscalMonthId: number
  rows: MonthSummaryRow[]
  collectionTotal: number
  expenditureTotal: number
  transferBalance: number
}

export interface AuditLog {
  id: number
  at: string
  kind: string
  targetTable: string
  targetId: number | null
  summary: string
}

export interface Counts {
  programs: number
  uploads: number
  entries: number
  students: number
}

export interface Bootstrap {
  app: AppInfo
  years: SchoolYear[]
  currentYear: SchoolYear | null
  schoolName: string
  counts: Counts
}

export interface AppError {
  code: string
  message: string
  detail?: string | null
}

/* ── P2: 배분 · 대조 ─────────────────────────────────── */

export interface Allocation {
  id: number
  entryId: number
  programId: number
  programName: string
  amount: number
  method: 'auto_single' | 'auto_split' | 'manual'
  status: 'active' | 'voided'
  confirmed: boolean
  reason: string
  createdAt: string
  sourceRowId: number | null
}

export interface AllocCandidate {
  programId: number
  programName: string
  itemCode: string
  amount: number
  sourceUploadId: number
  sourceRowId: number
  sameStudent: boolean
}

export type AllocPlan = 'auto' | 'manual' | 'name_only' | 'no_roster' | 'no_student' | 'done'

export interface PendingEntry {
  entryId: number
  entryKind: string
  student: string
  studentId: number | null
  fundName: string
  itemCode: string
  itemName: string
  amount: number
  allocated: number
  remaining: number
  plan: AllocPlan
  candidates: AllocCandidate[]
  allocations: Allocation[]
  fileName: string
  versionNo: number
}

export interface AllocationBoard {
  fiscalMonthId: number
  rosterUploadCount: number
  entries: PendingEntry[]
  unallocatedTotal: number
  allocatedTotal: number
  autoPossible: number
  needUser: number
  noStudent: number
  byItem: SummaryLine[]
}

export interface AutoAllocateResult {
  entriesAllocated: number
  lines: number
  skipped: number
}

export interface ManualPart {
  programId: number
  amount: number
}

export type ReconStatus = 'ok' | 'mismatch' | 'missing' | 'unallocated' | 'unmapped' | 'unverifiable' | 'group_only'
export type NoteStatus = 'none' | 'todo' | 'investigating' | 'explained'

export interface FundLine {
  fundId: number | null
  fundName: string
  rootId: number | null
  basis: number | null
  expense: number | null
  diff: number | null
  state: 'ok' | 'mismatch' | 'no_basis' | 'no_expense' | 'unmapped'
}

export interface ExcludedFund {
  fundName: string
  basis: number
  expense: number
  reason: string
}

export interface ReconRow {
  unitKind: 'program' | 'group'
  unitId: number
  unitName: string
  itemSet: string
  itemLabel: string
  initial: number
  extra: number
  refund: number
  allocatedIn: number
  basis: number
  expense: number
  simpleDiff: number
  comparableBasis: number
  comparableExpense: number
  /** 재원별 대조 차액 (판정 기준) */
  diff: number
  excludedBasis: number
  excludedExpense: number
  unmappedBasis: number
  unmappedExpense: number
  unallocatedInSet: number
  excluded: ExcludedFund[]
  totalMatchFundMismatch: boolean
  status: ReconStatus
  statusLabel: string
  hints: string[]
  funds: FundLine[]
  fundsMerged: FundLine[]
  noteStatus: NoteStatus
  noteLabel: string
  needsRecheck: boolean
  fingerprint: string
}

export interface MonthRecon {
  fiscalMonthId: number
  collectionTotal: number
  expenditureTotal: number
  simpleDiff: number
  comparableBasis: number
  comparableExpense: number
  excludedBasis: number
  excludedExpense: number
  unallocatedTotal: number
  todoCount: number
  mismatchCount: number
  missingCount: number
  rows: ReconRow[]
}

export interface TraceEntry {
  entryId: number
  entryKind: string
  kindLabel: string
  programName: string
  party: string
  fundName: string
  fundText: string
  itemName: string
  amount: number
  attributed: number
  viaAllocation: boolean
  monthLabel: string
  processedOn: string | null
  state: 'active' | 'voided' | 'superseded'
  fileId: number | null
  fileName: string
  versionNo: number
  fileStatus: string
  sheet: string
  rowNo: number
  sourceRowId: number | null
}

export interface ReconNote {
  id: number
  status: NoteStatus
  reason: string
  fingerprint: string
  createdAt: string
}

export interface ReconDetail {
  row: ReconRow
  monthLabel: string
  members: string[]
  collection: TraceEntry[]
  expenditure: TraceEntry[]
  parties: SummaryLine[]
  notes: ReconNote[]
}

export interface SourceRowView {
  fileId: number
  fileName: string
  versionNo: number
  storedName: string
  sheet: string
  rowNo: number
  cells: string[]
}
