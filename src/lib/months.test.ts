import { describe, expect, it } from 'vitest'

import { currentYm, labelOf, monthsOf, schoolYearOf } from './months'

describe('monthsOf', () => {
  it('3월부터 다음 해 2월까지 12개', () => {
    const m = monthsOf(2026)
    expect(m).toHaveLength(12)
    expect(m[0]).toEqual({ seq: 1, ym: '2026-03', label: '3월' })
    expect(m[9]).toEqual({ seq: 10, ym: '2026-12', label: '12월' })
    expect(m[10]).toEqual({ seq: 11, ym: '2027-01', label: '1월' })
    expect(m[11]).toEqual({ seq: 12, ym: '2027-02', label: '2월' })
  })
})

describe('schoolYearOf / currentYm / labelOf', () => {
  it('1·2월은 전년도 학년도', () => {
    expect(schoolYearOf(new Date(2026, 9, 13))).toBe(2026)
    expect(schoolYearOf(new Date(2027, 1, 28))).toBe(2026)
    expect(schoolYearOf(new Date(2027, 2, 1))).toBe(2027)
  })
  it('현재 달 표기', () => {
    expect(currentYm(new Date(2026, 9, 13))).toBe('2026-10')
    expect(labelOf('2027-02')).toBe('2월')
    expect(labelOf('bad')).toBe('bad')
  })
})
