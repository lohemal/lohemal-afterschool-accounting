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
  sortOrder: number
  isBuiltin: boolean
  isActive: boolean
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
}

export type UploadStatus = 'registered' | 'previewed' | 'applied' | 'cancelled' | 'replaced'

export interface UploadFile {
  id: number
  schoolYearId: number
  fileName: string
  sha256: string
  byteSize: number
  formatCode: string
  versionNo: number
  status: UploadStatus
  replacedById: number | null
  note: string
  uploadedAt: string
  appliedAt: string | null
  cancelledAt: string | null
  rowCount: number
  entryCount: number
}

export type Flow = 'collection' | 'expenditure' | 'transfer'
export type Stage = 'planned' | 'actual'

export interface MonthSummaryRow {
  programId: number
  programName: string
  fundTypeId: number
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
