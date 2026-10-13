import { Fragment, useState } from 'react'
import { useNavigate } from 'react-router-dom'

import { diff, won } from '@/lib/format'
import type { ReconRow, ReconStatus } from '@/ipc/types'

export const STATUS_CLS: Record<ReconStatus, string> = {
  ok: 'badge badge--ok',
  mismatch: 'badge badge--bad',
  missing: 'badge badge--muted',
  unallocated: 'badge badge--soon',
  unmapped: 'badge badge--soon',
  link_pending: 'badge badge--soon',
  unverifiable: 'badge badge--muted',
  group_only: 'badge badge--navy',
  orphan: 'badge badge--bad',
}

export function StatusBadge({ row }: { row: ReconRow }) {
  if (row.totalMatchFundMismatch) return <span className="badge badge--soon">🟡 총액 일치 / 재원 불일치</span>
  return <span className={STATUS_CLS[row.status]}>{row.statusLabel}</span>
}

export function NoteBadge({ row }: { row: ReconRow }) {
  if (row.needsRecheck) return <span className="badge badge--bad">재확인 필요</span>
  if (row.noteStatus === 'none') {
    return ['mismatch', 'unallocated', 'unmapped', 'link_pending'].includes(row.status) ? <span className="badge badge--soon">확인 필요</span> : <span className="muted">—</span>
  }
  const cls = row.noteStatus === 'explained' ? 'badge badge--ok' : row.noteStatus === 'investigating' ? 'badge badge--navy' : 'badge badge--soon'
  return <span className={cls}>{row.noteLabel}</span>
}

/** 프로그램 × 항목 대조 표 (월별 정산·차액 분석 공용). 통합 프로그램 행은 펼쳐서 세부반 징수를 보여 준다 — 지급은 통합 행에만. */
export function ReconTable({ rows, onOpen }: { rows: ReconRow[]; onOpen: (r: ReconRow) => void }) {
  const [open, setOpen] = useState<Record<string, boolean>>({})
  const navigate = useNavigate()
  return (
    <div className="tableWrap">
      <table className="table">
        <thead>
          <tr>
            <th>프로그램</th>
            <th>항목</th>
            <th className="num">최초 징수</th>
            <th className="num">추가징수</th>
            <th className="num">환불</th>
            <th className="num">징수 기준액</th>
            <th className="num">지급액</th>
            <th className="num" title="전체 징수 기준액 − 전체 지급액. 비교 범위가 다르면 오류가 아닙니다.">단순 차액</th>
            <th className="num" title="양쪽 자료가 다 있는 재원끼리의 차액. 판정 기준.">대조 차액</th>
            <th className="center">검증</th>
            <th className="center">확인 상태</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => {
            const key = `${r.unitKind}-${r.unitId}-${r.itemSet}`
            const expandable = r.children.length > 0
            const isOpen = !!open[key]
            return (
              <Fragment key={key}>
                <tr className={r.status === 'mismatch' ? 'row--error' : r.status === 'unallocated' || r.status === 'unmapped' || r.status === 'link_pending' ? 'row--check' : ''}>
                  <td>
                    {expandable ? (
                      <button type="button" className="linkbtn" title={isOpen ? '세부반 접기' : '세부반 펼치기'} onClick={() => setOpen({ ...open, [key]: !isOpen })}>
                        {isOpen ? '▾' : '▸'} {r.unitName}
                      </button>
                    ) : (
                      r.unitName
                    )}
                    {r.isParent && <span className="badge badge--navy" style={{ marginLeft: 6 }}>통합</span>}
                  </td>
                  <td>{r.itemLabel}</td>
                  <td className="num">{won(r.initial)}</td>
                  <td className="num">{r.extra ? won(r.extra) : <span className="muted">—</span>}</td>
                  <td className="num">{r.refund ? won(r.refund) : <span className="muted">—</span>}</td>
                  <td className="num">{won(r.basis)}</td>
                  <td className="num">{won(r.expense)}</td>
                  <td className="num muted">{r.status === 'link_pending' ? '—' : diff(r.simpleDiff)}</td>
                  <td className={`num ${r.status === 'ok' || r.status === 'mismatch' ? (r.diff > 0 ? 'diffcell--up' : r.diff < 0 ? 'diffcell--down' : '') : 'muted'}`}>
                    {r.status === 'ok' || r.status === 'mismatch' ? diff(r.diff) : '—'}
                    {(r.excludedBasis !== 0 || r.excludedExpense !== 0) && (r.status === 'ok' || r.status === 'mismatch') && <span className="muted" style={{ fontSize: 10, marginLeft: 4 }}>제외 {won(r.excludedBasis + r.excludedExpense)}</span>}
                  </td>
                  <td className="center">
                    <StatusBadge row={r} />
                  </td>
                  <td className="center">
                    <NoteBadge row={r} />
                  </td>
                  <td>
                    <span className="row" style={{ gap: 4 }}>
                      {r.status === 'link_pending' && (
                        <button type="button" className="btn btn--sm" title={r.linkHint} onClick={() => navigate('/programs')}>
                          매핑 관리
                        </button>
                      )}
                      <button type="button" className="btn btn--sm" onClick={() => onOpen(r)}>
                        상세
                      </button>
                    </span>
                  </td>
                </tr>
                {expandable &&
                  isOpen &&
                  r.children.map((c) => (
                    <tr key={`${key}-c-${c.programId}-${c.direct ? 'd' : 'c'}`} className="row--off">
                      <td style={{ paddingLeft: 28 }}>
                        <span className="muted">└</span> {c.programName}
                        {c.direct && <span className="badge badge--muted" style={{ marginLeft: 6 }}>직접 귀속</span>}
                      </td>
                      <td className="muted">{r.itemLabel}</td>
                      <td className="num">{won(c.initial)}</td>
                      <td className="num">{c.extra ? won(c.extra) : <span className="muted">—</span>}</td>
                      <td className="num">{c.refund ? won(c.refund) : <span className="muted">—</span>}</td>
                      <td className="num">{won(c.basis)}</td>
                      <td className="num muted" title="지급은 통합 프로그램 행에만 있습니다 (세부반에 다시 표시하지 않음)">{c.expense ? `${won(c.expense)} (직접 지급 · 확인)` : '—'}</td>
                      <td className="num muted">—</td>
                      <td className="num muted">—</td>
                      <td className="center"><span className="badge badge--muted">세부반 · 합산됨</span></td>
                      <td className="center muted">{won(c.entryCount)}건</td>
                      <td />
                    </tr>
                  ))}
              </Fragment>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}
