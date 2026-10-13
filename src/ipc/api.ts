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
  FiscalMonth,
  FundType,
  MonthSummary,
  Program,
  SchoolYear,
  UploadFile,
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

  catalogList: () => invoke<Catalog>('catalog_list'),
  fundTypeSave: (input: CatalogInput) => invoke<FundType>('fund_type_save', { input }),
  amountItemSave: (input: CatalogInput) => invoke<AmountItem>('amount_item_save', { input }),

  monthSummary: (fiscalMonthId: number) => invoke<MonthSummary>('month_summary', { fiscalMonthId }),
  programList: (schoolYearId: number) => invoke<Program[]>('program_list', { schoolYearId }),
  uploadList: (schoolYearId: number) => invoke<UploadFile[]>('upload_list', { schoolYearId }),
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
