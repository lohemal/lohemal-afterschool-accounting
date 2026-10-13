import { diff, won } from '@/lib/format'
import type { ReconRow, ReconStatus } from '@/ipc/types'

export const STATUS_CLS: Record<ReconStatus, string> = {
  ok: 'badge badge--ok',
  mismatch: 'badge badge--bad',
  missing: 'badge badge--muted',
  unallocated: 'badge badge--soon',
  unmapped: 'badge badge--soon',
  unverifiable: 'badge badge--muted',
  group_only: 'badge badge--navy',
}

export function StatusBadge({ row }: { row: ReconRow }) {
  return <span className={STATUS_CLS[row.status]}>{row.statusLabel}</span>
}

export function NoteBadge({ row }: { row: ReconRow }) {
  if (row.needsRecheck) return <span className="badge badge--bad">재확인 필요</span>
  if (row.noteStatus === 'none') {
    return ['mismatch', 'unallocated', 'unmapped'].includes(row.status) ? <span className="badge badge--soon">확인 필요</span> : <span className="muted">—</span>
  }
  const cls = row.noteStatus === 'explained' ? 'badge badge--ok' : row.noteStatus === 'investigating' ? 'badge badge--navy' : 'badge badge--soon'
  return <span className={cls}>{row.noteLabel}</span>
}

/** 프로그램 × 항목 대조 표 (월별 정산·차액 분석 공용). */
export function ReconTable({ rows, onOpen }: { rows: ReconRow[]; onOpen: (r: ReconRow) => void }) {
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
            <th className="num">차액</th>
            <th className="center">검증</th>
            <th className="center">확인 상태</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {rows.map((r) => (
            <tr key={`${r.unitKind}-${r.unitId}-${r.itemSet}`} className={r.status === 'mismatch' ? 'row--error' : r.status === 'unallocated' || r.status === 'unmapped' ? 'row--check' : ''}>
              <td>{r.unitName}</td>
              <td>{r.itemLabel}</td>
              <td className="num">{won(r.initial)}</td>
              <td className="num">{r.extra ? won(r.extra) : <span className="muted">—</span>}</td>
              <td className="num">{r.refund ? won(r.refund) : <span className="muted">—</span>}</td>
              <td className="num">{won(r.basis)}</td>
              <td className="num">{won(r.expense)}</td>
              <td className={`num ${r.status === 'ok' || r.status === 'mismatch' ? (r.diff > 0 ? 'diffcell--up' : r.diff < 0 ? 'diffcell--down' : '') : 'muted'}`}>
                {r.status === 'ok' || r.status === 'mismatch' ? diff(r.diff) : '—'}
              </td>
              <td className="center">
                <StatusBadge row={r} />
              </td>
              <td className="center">
                <NoteBadge row={r} />
              </td>
              <td>
                <button type="button" className="btn btn--sm" onClick={() => onOpen(r)}>
                  상세
                </button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}
