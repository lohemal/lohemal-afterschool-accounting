import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Empty, Notice, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import type { PendingEntry } from '@/ipc/types'

const PLAN_LABEL: Record<string, [string, string]> = {
  auto: ['자동 배분 가능', 'badge badge--ok'],
  manual: ['사용자 확인 필요', 'badge badge--soon'],
  name_only: ['동명이인 · 확인 필요', 'badge badge--soon'],
  no_roster: ['참고 명단에 없음', 'badge badge--muted'],
  no_student: ['학생 식별 불가', 'badge badge--bad'],
  done: ['배분 완료', 'badge badge--navy'],
}

/** 프로그램별 징수액 배분 — 미배분 거래를 참고 명단으로 프로그램에 잇는다. */
export function AllocationPanel({ fiscalMonthId, yearId, closed }: { fiscalMonthId: number; yearId: number; closed: boolean }) {
  const qc = useQueryClient()
  const toast = useToast()
  const board = useQuery({ queryKey: ['allocBoard', fiscalMonthId], queryFn: () => api.allocationBoard(fiscalMonthId) })
  const [filter, setFilter] = useState<'open' | 'all'>('open')
  const [manual, setManual] = useState<PendingEntry | null>(null)
  const [busy, setBusy] = useState(false)

  async function runAuto() {
    setBusy(true)
    try {
      const r = await api.allocationAuto(fiscalMonthId)
      toast.show(`자동 배분 ${won(r.entriesAllocated)}건 (${won(r.lines)}줄) · 보류 ${won(r.skipped)}건`)
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  async function voidAlloc(id: number) {
    const reason = window.prompt('배분 취소 사유를 입력해 주세요.')
    if (!reason) return
    try {
      await api.allocationVoid(id, reason)
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }

  const b = board.data
  const shown = (b?.entries ?? []).filter((e) => filter === 'all' || e.remaining !== 0)
  return (
    <>
      {board.error && <Notice kind="bad">{errorMessage(board.error)}</Notice>}
      {b && (
        <>
          <div className="row">
            <div className="stat"><span>미배분 징수 거래</span><strong>{won(b.entries.filter((e) => e.remaining !== 0).length)}건</strong></div>
            <div className={`stat ${b.unallocatedTotal ? 'stat--warn' : ''}`}><span>미배분 금액</span><strong>{won(b.unallocatedTotal)}</strong></div>
            <div className="stat"><span>배분 완료 금액</span><strong>{won(b.allocatedTotal)}</strong></div>
            <div className="stat"><span>자동 배분 가능</span><strong>{won(b.autoPossible)}건</strong></div>
            <div className="stat"><span>사용자 확인 필요</span><strong>{won(b.needUser)}건</strong></div>
            <div className={`stat ${b.noStudent ? 'stat--bad' : ''}`}><span>학생 식별 불가</span><strong>{won(b.noStudent)}건</strong></div>
            <div className="stat"><span>참고 명단 파일</span><strong>{won(b.rosterUploadCount)}개</strong></div>
            <span style={{ marginLeft: 'auto' }} />
            <select className="select input--sm" value={filter} onChange={(e) => setFilter(e.target.value as 'open' | 'all')}>
              <option value="open">미배분만</option>
              <option value="all">전체</option>
            </select>
            <Button variant="primary" disabled={busy || closed || b.autoPossible === 0} onClick={runAuto}>
              자동 배분 실행 ({won(b.autoPossible)}건)
            </Button>
          </div>
          {closed && <Notice>확정된 달입니다. 배분을 바꾸려면 월별 정산에서 재개해 주세요.</Notice>}
          {b.rosterUploadCount === 0 && b.entries.length > 0 && (
            <Notice>이 달에 적용된 프로그램별 수강생명단(자료 유형 '프로그램 연결·검증')이 없습니다. 자료 업로드에서 명단을 올리면 배분 후보가 나타납니다.</Notice>
          )}
          <p className="muted" style={{ fontSize: 12 }}>
            자동 배분은 학년·반·번호·이름이 모두 같은 학생이 참고 명단에서 한 프로그램뿐이거나, 여러 프로그램이라도 명단의 프로그램별 금액 합이 징수 금액과 정확히 같을 때만 합니다. 동명이인·식별 불가·금액 불일치는 사용자가 확인합니다. 배분은 원장 금액을 바꾸지 않습니다.
          </p>
          {shown.length === 0 ? (
            <Empty title={filter === 'open' ? '미배분 징수 거래가 없습니다.' : '배분 대상 거래가 없습니다.'} />
          ) : (
            <div className="tableWrap" style={{ maxHeight: 520 }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>학생</th>
                    <th>항목</th>
                    <th>재원</th>
                    <th className="num">징수액</th>
                    <th className="num">배분됨</th>
                    <th className="num">미배분</th>
                    <th>후보 프로그램 (참고 명단)</th>
                    <th className="center">판정</th>
                    <th>원본</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {shown.map((e) => {
                    const [label, cls] = PLAN_LABEL[e.plan] ?? [e.plan, 'badge badge--muted']
                    return (
                      <tr key={e.entryId} className={e.plan === 'no_student' ? 'row--error' : e.remaining !== 0 && e.plan !== 'auto' ? 'row--check' : ''}>
                        <td>{e.student || <span className="muted">(식별 불가)</span>}</td>
                        <td>{e.itemName}</td>
                        <td>{e.fundName}</td>
                        <td className="num">{won(e.amount)}</td>
                        <td className="num">{won(e.allocated)}</td>
                        <td className="num">{won(e.remaining)}</td>
                        <td className="wrap" style={{ fontSize: 12 }}>
                          {e.candidates.length === 0 ? <span className="muted">—</span> : e.candidates.map((c, i) => (
                            <span key={i} className="chip" style={{ marginRight: 4 }}>
                              {c.programName}{c.amount ? ` ${won(c.amount)}` : ''}{c.sameStudent ? '' : ' (이름만 같음)'}
                            </span>
                          ))}
                          {e.allocations.filter((a) => a.status === 'active').map((a) => (
                            <span key={a.id} className="chip" style={{ marginRight: 4, background: 'var(--teal-50)' }}>
                              → {a.programName} {won(a.amount)} ({a.method === 'manual' ? '수동' : '자동'})
                              {!closed && <button type="button" title="배분 취소" onClick={() => voidAlloc(a.id)}>×</button>}
                            </span>
                          ))}
                        </td>
                        <td className="center"><span className={cls}>{label}</span></td>
                        <td className="muted" style={{ fontSize: 11 }}>{e.fileName}{e.versionNo ? ` (제${e.versionNo}판)` : ''}</td>
                        <td>{e.remaining !== 0 && !closed && <Button size="sm" onClick={() => setManual(e)}>직접 배분…</Button>}</td>
                      </tr>
                    )
                  })}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
      {manual && <ManualAllocModal entry={manual} yearId={yearId} onClose={() => setManual(null)} onDone={() => { setManual(null); void qc.invalidateQueries() }} />}
    </>
  )
}

function ManualAllocModal({ entry, yearId, onClose, onDone }: { entry: PendingEntry; yearId: number; onClose: () => void; onDone: () => void }) {
  const toast = useToast()
  const programs = useQuery({ queryKey: ['programs', yearId], queryFn: () => api.programList(yearId) })
  const init = entry.candidates.length > 0 ? entry.candidates.map((c) => ({ programId: c.programId, amount: entry.candidates.length === 1 ? entry.remaining : c.amount })) : [{ programId: 0, amount: entry.remaining }]
  const [parts, setParts] = useState(init)
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const total = parts.reduce((s, p) => s + (Number.isInteger(p.amount) ? p.amount : 0), 0)
  const over = Math.abs(total) > Math.abs(entry.remaining) || parts.some((p) => (p.amount > 0) !== (entry.amount > 0))
  async function save() {
    setBusy(true)
    try {
      await api.allocationManual(entry.entryId, parts.filter((p) => p.programId && p.amount !== 0), reason)
      toast.show('배분을 저장했습니다.')
      onDone()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }
  return (
    <Modal
      title="직접 배분"
      onClose={onClose}
      width={680}
      footer={
        <>
          <Button onClick={onClose}>취소</Button>
          <Button variant="primary" disabled={busy || over || total === 0 || parts.some((p) => !p.programId) || !reason.trim()} onClick={save}>
            배분 저장
          </Button>
        </>
      }
    >
      <div className="row">
        <div className="stat"><span>학생</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{entry.student || '(식별 불가)'}</strong></div>
        <div className="stat"><span>항목 · 재원</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{entry.itemName} · {entry.fundName}</strong></div>
        <div className="stat"><span>원본 징수액</span><strong>{won(entry.amount)}</strong></div>
        <div className="stat"><span>미배분 잔액</span><strong>{won(entry.remaining)}</strong></div>
        <div className={`stat ${over ? 'stat--bad' : ''}`}><span>배분 예정 합</span><strong>{won(total)}</strong></div>
      </div>
      {over && <Notice kind="bad">배분 합이 미배분 잔액을 넘거나 부호가 다릅니다. 저장되지 않습니다.</Notice>}
      <div className="tableWrap" style={{ marginTop: 10 }}>
        <table className="table">
          <thead>
            <tr>
              <th>프로그램</th>
              <th className="num">배분 금액</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {parts.map((p, i) => (
              <tr key={i}>
                <td>
                  <select className="select input--sm" value={p.programId} onChange={(e) => setParts(parts.map((x, j) => (j === i ? { ...x, programId: Number(e.target.value) } : x)))}>
                    <option value={0}>— 선택 —</option>
                    {(programs.data ?? []).map((pr) => (
                      <option key={pr.id} value={pr.id}>
                        {pr.name}
                      </option>
                    ))}
                  </select>
                </td>
                <td className="num">
                  <input className="input input--sm" type="number" step={1} value={p.amount} onChange={(e) => setParts(parts.map((x, j) => (j === i ? { ...x, amount: Math.trunc(Number(e.target.value)) } : x)))} style={{ width: 140, textAlign: 'right' }} />
                </td>
                <td>
                  <Button size="sm" onClick={() => setParts(parts.filter((_, j) => j !== i))}>삭제</Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="row" style={{ marginTop: 8 }}>
        <Button size="sm" onClick={() => setParts([...parts, { programId: 0, amount: entry.remaining - total }])}>프로그램 추가</Button>
        <span className="muted" style={{ fontSize: 12 }}>남는 금액은 미배분으로 둡니다. 원 단위 정수만.</span>
      </div>
      <div className="row" style={{ marginTop: 10 }}>
        <span className="label">사유</span>
        <input className="input" style={{ flex: 1 }} placeholder="예: 담당자가 수강 명단으로 확인" value={reason} onChange={(e) => setReason(e.target.value)} />
      </div>
    </Modal>
  )
}
