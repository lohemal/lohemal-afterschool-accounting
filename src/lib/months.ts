/** 학년도 ↔ 회계월 (3월 ~ 다음 해 2월). Rust `domain/months.rs` 와 같은 규칙. */

export interface YmInfo {
  seq: number
  ym: string
  label: string
}

export function monthsOf(year: number): YmInfo[] {
  const out: YmInfo[] = []
  for (let seq = 1; seq <= 12; seq++) {
    const month = ((seq + 1) % 12) + 1
    const y = seq <= 10 ? year : year + 1
    out.push({ seq, ym: `${y}-${String(month).padStart(2, '0')}`, label: `${month}월` })
  }
  return out
}

/** 오늘 날짜가 속한 학년도. 1·2월은 전년도 학년도다. */
export function schoolYearOf(date: Date): number {
  const m = date.getMonth() + 1
  return m >= 3 ? date.getFullYear() : date.getFullYear() - 1
}

/** 'YYYY-MM' 이 오늘 기준 어느 달인지 — 학년도 안에서 기본 선택 달을 고를 때 쓴다. */
export function currentYm(date: Date): string {
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, '0')}`
}

export function labelOf(ym: string): string {
  const m = Number(ym.slice(5, 7))
  return Number.isInteger(m) && m >= 1 && m <= 12 ? `${m}월` : ym
}
