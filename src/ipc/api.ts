/**
 * Rust 명령 호출 묶음. 화면에서는 `invoke('...')` 를 직접 쓰지 않고 여기만 쓴다.
 */

import { invoke } from '@tauri-apps/api/core'

import type {
  AmountItem,
  AppError,
  AuditLog,
  Bootstrap,
  Catalog,
  CatalogInput,
  CommitResult,
  FiscalMonth,
  FundAlias,
  FundType,
  Impact,
  Inspect,
  MonthSummary,
  Preview,
  PreviewRequest,
  Program,
  ProgramGroup,
  ProgramMapInput,
  SchoolYear,
  UploadDetail,
  UploadFile,
  AllocationBoard,
  AutoAllocateResult,
  ManualPart,
  MonthRecon,
  ReconDetail,
  ReconNote,
  SourceRowView,
} from './types'

export const api = {
  bootstrap: () => invoke<Bootstrap>('bootstrap'),
  settingGet: (key: string) => invoke<string | null>('setting_get', { key }),
  settingSet: (key: string, value: string) => invoke<void>('setting_set', { key, value }),
  auditList: (limit?: number) => invoke<AuditLog[]>('audit_list', { limit }),
  openDataFolder: () => invoke<void>('open_data_folder'),

  yearList: () => invoke<SchoolYear[]>('year_list'),
  yearCreate: (year: number) => invoke<SchoolYear>('year_create', { year }),
  yearSetCurrent: (id: number) => invoke<SchoolYear>('year_set_current', { id }),
  monthList: (schoolYearId: number) => invoke<FiscalMonth[]>('month_list', { schoolYearId }),
  monthClose: (fiscalMonthId: number, note?: string) => invoke<void>('month_close', { fiscalMonthId, note }),
  monthReopen: (fiscalMonthId: number, note: string) => invoke<void>('month_reopen', { fiscalMonthId, note }),

  catalogList: () => invoke<Catalog>('catalog_list'),
  fundTypeSave: (input: CatalogInput) => invoke<FundType>('fund_type_save', { input }),
  amountItemSave: (input: CatalogInput) => invoke<AmountItem>('amount_item_save', { input }),
  fundAliasList: () => invoke<FundAlias[]>('fund_alias_list'),
  fundAliasSave: (fundTypeId: number, alias: string) => invoke<FundAlias>('fund_alias_save', { fundTypeId, alias }),
  fundAliasDelete: (id: number) => invoke<void>('fund_alias_delete', { id }),

  monthSummary: (fiscalMonthId: number) => invoke<MonthSummary>('month_summary', { fiscalMonthId }),
  programList: (schoolYearId: number) => invoke<Program[]>('program_list', { schoolYearId }),
  programGroupList: (schoolYearId: number) => invoke<ProgramGroup[]>('program_group_list', { schoolYearId }),
  programMapSave: (input: ProgramMapInput) => invoke<void>('program_map_save', { input }),
  programAliasDelete: (schoolYearId: number, alias: string) => invoke<void>('program_alias_delete', { schoolYearId, alias }),

  uploadKinds: () => invoke<[string, string][]>('upload_kinds'),
  uploadInspect: (schoolYearId: number, path: string) => invoke<Inspect>('upload_inspect', { schoolYearId, path }),
  uploadPreview: (req: PreviewRequest) => invoke<Preview>('upload_preview', { req }),
  uploadCommit: (req: PreviewRequest) => invoke<CommitResult>('upload_commit', { req }),
  uploadList: (schoolYearId: number) => invoke<UploadFile[]>('upload_list', { schoolYearId }),
  uploadDetail: (fileId: number) => invoke<UploadDetail>('upload_detail', { fileId }),
  uploadImpact: (fileId: number, action: 'cancel' | 'restore') => invoke<Impact>('upload_impact', { fileId, action }),
  uploadCancel: (fileId: number, reason: string) => invoke<UploadFile>('upload_cancel', { fileId, reason }),
  uploadRestore: (fileId: number, reason: string) => invoke<UploadFile>('upload_restore', { fileId, reason }),

  allocationBoard: (fiscalMonthId: number) => invoke<AllocationBoard>('allocation_board', { fiscalMonthId }),
  allocationAuto: (fiscalMonthId: number) => invoke<AutoAllocateResult>('allocation_auto', { fiscalMonthId }),
  allocationManual: (entryId: number, parts: ManualPart[], reason: string) => invoke<number[]>('allocation_manual', { entryId, parts, reason }),
  allocationVoid: (allocationId: number, reason: string) => invoke<void>('allocation_void', { allocationId, reason }),
  reconMonth: (fiscalMonthId: number) => invoke<MonthRecon>('recon_month', { fiscalMonthId }),
  reconDetail: (fiscalMonthId: number, unitKind: string, unitId: number, itemSet: string) => invoke<ReconDetail>('recon_detail', { fiscalMonthId, unitKind, unitId, itemSet }),
  reconNoteSave: (fiscalMonthId: number, unitKind: string, unitId: number, itemSet: string, status: string, reason: string) => invoke<ReconNote>('recon_note_save', { fiscalMonthId, unitKind, unitId, itemSet, status, reason }),
  sourceRowView: (sourceRowId: number) => invoke<SourceRowView>('source_row_view', { sourceRowId }),
  openStoredFile: (fileId: number) => invoke<void>('open_stored_file', { fileId }),
}

export function isAppError(e: unknown): e is AppError {
  return typeof e === 'object' && e !== null && 'code' in e && 'message' in e
}

/** 오류를 화면에 보여줄 한 줄로. Rust 의 AppError 면 한국어 문장을 그대로 쓴다. */
export function errorMessage(e: unknown): string {
  if (isAppError(e)) return e.message
  if (e instanceof Error) return e.message
  if (typeof e === 'string') return e
  return '알 수 없는 오류가 났습니다.'
}
