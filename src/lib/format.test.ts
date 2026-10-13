import { describe, expect, it } from 'vitest'

import { bytes, diff, shortDateTime, won } from './format'

describe('won', () => {
  it('천 단위 구분기호를 붙인다', () => {
    expect(won(0)).toBe('0')
    expect(won(999)).toBe('999')
    expect(won(1000)).toBe('1,000')
    expect(won(27329000)).toBe('27,329,000')
    expect(won(-330850)).toBe('-330,850')
  })
  it('없는 값은 대시로, 정수가 아닌 값은 숨기지 않고 드러낸다', () => {
    expect(won(null)).toBe('—')
    expect(won(undefined)).toBe('—')
    expect(won(3.5)).toBe('3.5 (정수 아님)')
  })
})

describe('diff', () => {
  it('부호를 명시한다', () => {
    expect(diff(0)).toBe('0')
    expect(diff(18000)).toBe('+18,000')
    expect(diff(-42000)).toBe('-42,000')
  })
})

describe('shortDateTime / bytes', () => {
  it('초를 뗀다', () => {
    expect(shortDateTime('2026-10-13 09:12:33')).toBe('2026-10-13 09:12')
    expect(shortDateTime(null)).toBe('—')
  })
  it('크기 단위', () => {
    expect(bytes(500)).toBe('500 B')
    expect(bytes(53015)).toBe('51.8 KB')
    expect(bytes(3 * 1024 * 1024)).toBe('3.0 MB')
  })
})
