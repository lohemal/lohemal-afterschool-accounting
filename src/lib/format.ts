/** 금액·날짜 표시. 금액은 원 단위 정수만 받는다 — 소수가 오면 버그이므로 그대로 드러낸다. */

export function won(v: number | null | undefined): string {
  if (v === null || v === undefined) return '—'
  if (!Number.isInteger(v)) return `${v} (정수 아님)`
  const sign = v < 0 ? '-' : ''
  const digits = Math.abs(v).toString()
  let out = ''
  for (let i = 0; i < digits.length; i++) {
    if (i > 0 && (digits.length - i) % 3 === 0) out += ','
    out += digits[i]
  }
  return sign + out
}

/** 차액 표시 — 0 이면 '0', 양수는 '+', 음수는 '-'. */
export function diff(v: number): string {
  if (v === 0) return '0'
  return v > 0 ? `+${won(v)}` : won(v)
}

/** 'YYYY-MM-DD HH:MM:SS' → 'YYYY-MM-DD HH:MM' */
export function shortDateTime(s: string | null | undefined): string {
  if (!s) return '—'
  return s.length >= 16 ? s.slice(0, 16) : s
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`
  return `${(n / 1024 / 1024).toFixed(1)} MB`
}
