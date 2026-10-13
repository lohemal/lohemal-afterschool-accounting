import { useMemo, useState } from 'react'

import { won } from '@/lib/format'
import type { PreviewRow } from '@/ipc/types'

const KIND_LABEL: Record<string, string> = {
  initial_collect: '최초 징수',
  extra_collect: '추가징수',
  refund: '환불',
  instructor_pay: '강사료 지급',
  vendor_pay: '교재·재료비 지급',
}

const STATUS_LABEL: Record<string, string> = { ok: '정상', check: '확인 필요', error: '오류', skip: '제외' }

/** 미리보기·상세 공용 행 표. 상태로 거를 수 있고 300행까지만 그린다. */
export function RowsTable({ rows, children }: { rows: PreviewRow[]; children?: (r: PreviewRow) => React.ReactNode }) {
  const [filter, setFilter] = useState<'all' | 'check' | 'error' | 'off'>('all')
  const [limit, setLimit] = useState(300)
  const shown = useMemo(() => {
    const f = rows.filter((r) =>
      filter === 'all' ? true : filter === 'off' ? !r.included : r.status === filter,
    )
    return f
  }, [rows, filter])
  return (
    <>
      <div className="row" style={{ marginBottom: 6 }}>
        <select className="select input--sm" value={filter} onChange={(e) => setFilter(e.target.value as typeof filter)}>
          <option value="all">전체 {rows.length}행</option>
          <option value="check">확인 필요 {rows.filter((r) => r.status === 'check').length}행</option>
          <option value="error">오류 {rows.filter((r) => r.status === 'error').length}행</option>
          <option value="off">원장 제외 {rows.filter((r) => !r.included).length}행</option>
        </select>
        {shown.length > limit && (
          <span className="muted">
            {limit}행까지 표시 중 ·{' '}
            <a href="#" onClick={(e) => { e.preventDefault(); setLimit(limit + 500) }}>
              더 보기
            </a>
          </span>
        )}
      </div>
      <div className="tableWrap" style={{ maxHeight: 460 }}>
        <table className="table">
          <thead>
            <tr>
              <th>시트·행</th>
              <th>프로그램</th>
              <th>학생</th>
              <th>재원</th>
              <th>항목</th>
              <th>유형</th>
              <th className="num">금액</th>
              <th>상대</th>
              <th className="center">상태</th>
              <th>확인 사항</th>
              {children && <th />}
            </tr>
          </thead>
          <tbody>
            {shown.slice(0, limit).map((r) => (
              <tr key={`${r.sheet}-${r.rowNo}-${r.seq}`} className={r.status === 'error' ? 'row--error' : r.status === 'check' ? 'row--check' : !r.included ? 'row--off' : ''}>
                <td className="muted">
                  {r.sheet} {r.rowNo}
                </td>
                <td>
                  {r.programName || (r.programText ? <span className="muted">{r.programText}</span> : <span className="muted">—</span>)}
                  {r.programName && r.programText && r.programName !== r.programText && (
                    <span className="muted" style={{ marginLeft: 4, fontSize: 11 }}>
                      ({r.programText})
                    </span>
                  )}
                </td>
                <td>{r.student || <span className="muted">—</span>}</td>
                <td>
                  {r.fundName}
                  {r.fundText && r.fundText !== r.fundName && (
                    <span className="muted" style={{ marginLeft: 4, fontSize: 11 }}>
                      ({r.fundText.replace(/\s+/g, ' ')})
                    </span>
                  )}
                </td>
                <td>{r.itemName || <span className="muted">—</span>}</td>
                <td>{KIND_LABEL[r.entryKind] ?? (r.entryKind || <span className="muted">참고</span>)}</td>
                <td className="num">{r.entryKind ? won(r.amount) : <span className="muted">—</span>}</td>
                <td>{r.counterparty}</td>
                <td className="center">
                  <span className={`badge ${r.status === 'ok' ? 'badge--ok' : r.status === 'check' ? 'badge--soon' : r.status === 'error' ? 'badge--bad' : 'badge--muted'}`}>
                    {STATUS_LABEL[r.status]}
                    {!r.included && r.status !== 'error' ? ' · 제외' : ''}
                  </span>
                </td>
                <td className="wrap" style={{ fontSize: 12 }}>
                  {r.issues.filter((i) => i.level !== 'info').map((i) => i.message).join(' / ')}
                </td>
                {children && <td>{children(r)}</td>}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </>
  )
}
