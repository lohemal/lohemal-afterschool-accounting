import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'
import { useLocation } from 'react-router-dom'

import { Modal } from '@/components/Modal'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { diff, shortDateTime, won } from '@/lib/format'
import { useApp } from '@/lib/useApp'
import type { Adjustment, AdjustmentInput, AdjustmentPreview, AdjustKind } from '@/ipc/types'

type Tab = 'transfer' | 'refund' | 'extra' | 'history'

/** 차액 상세에서 넘어올 때의 미리 채움. */
interface Prefill {
  tab?: Tab
  fiscalMonthId?: number
  programId?: number
  itemSet?: string
  funds?: { fundId: number | null; fundName: string; diff: number | null }[]
}

const today = () => new Date().toISOString().slice(0, 10)

export function AdjustPage() {
  const { year } = useApp()
  const prefill = (useLocation().state ?? null) as Prefill | null
  const [tab, setTab] = useState<Tab>(prefill?.tab ?? 'transfer')

  if (!year) {
    return (
      <Page title="회계 정정">
        <Notice>학년도가 없습니다.</Notice>
      </Page>
    )
  }
  return (
    <Page title="회계 정정" sub="재원경정은 재원의 변경, 환불은 실제 반환 또는 수강료 감액, 추가징수는 새 징수액의 발생입니다. 기존 거래는 고치지 않고 새 기록으로 정정하며 취소·감사 추적이 됩니다.">
      <div className="tabs">
        {([
          ['transfer', '재원경정'],
          ['refund', '환불'],
          ['extra', '추가징수'],
          ['history', '정정 이력'],
        ] as [Tab, string][]).map(([k, l]) => (
          <button key={k} type="button" className={tab === k ? 'tab tab--on' : 'tab'} onClick={() => setTab(k)}>
            {l}
          </button>
        ))}
      </div>
      {tab === 'transfer' && <AdjustForm yearId={year.id} kind="fund_transfer" prefill={prefill} onDone={() => setTab('history')} />}
      {tab === 'refund' && <AdjustForm yearId={year.id} kind="refund" prefill={null} onDone={() => setTab('history')} />}
      {tab === 'extra' && <AdjustForm yearId={year.id} kind="extra_collect" prefill={null} onDone={() => setTab('history')} />}
      {tab === 'history' && <History yearId={year.id} />}
    </Page>
  )
}

/* ── 등록 폼 (재원경정 · 환불 · 추가징수 공용) ─────────────────────── */

function AdjustForm({ yearId, kind: baseKind, prefill, onDone }: { yearId: number; kind: AdjustKind; prefill: Prefill | null; onDone: () => void }) {
  const toast = useToast()
  const qc = useQueryClient()
  const months = useQuery({ queryKey: ['months', yearId], queryFn: () => api.monthList(yearId) })
  const programs = useQuery({ queryKey: ['programs', yearId], queryFn: () => api.programList(yearId) })
  const students = useQuery({ queryKey: ['students', yearId], queryFn: () => api.studentList(yearId) })
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })

  const [kind, setKind] = useState<AdjustKind>(baseKind)
  const [monthId, setMonthId] = useState<number | ''>(prefill?.fiscalMonthId ?? '')
  const [programId, setProgramId] = useState<number | ''>(prefill?.programId ?? '')
  const [studentId, setStudentId] = useState<number | ''>('')
  const [fromFund, setFromFund] = useState<number | ''>('')
  const [toFund, setToFund] = useState<number | ''>('')
  const [fund, setFund] = useState<number | ''>('')
  const [itemId, setItemId] = useState<number | ''>('')
  const [amount, setAmount] = useState<number>(0)
  const [decidedOn, setDecidedOn] = useState(today())
  const [processedOn, setProcessedOn] = useState(today())
  const [status, setStatus] = useState<'planned' | 'done'>('done')
  const [reason, setReason] = useState('')
  const [relatedEntry, setRelatedEntry] = useState<number | ''>('')
  const [relatedAdj, setRelatedAdj] = useState<number | ''>('')
  const [confirmDup, setConfirmDup] = useState(false)
  const [preview, setPreview] = useState<AdjustmentPreview | null>(null)
  const [previewErr, setPreviewErr] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)

  // 차액 상세에서 넘어온 재원별 차액을 후보로 보여 준다 (자동 적용하지 않는다).
  const hintFunds = prefill?.funds ?? []

  useEffect(() => {
    if (itemId === '' && cat.data) {
      const code = prefill?.itemSet === 'textbook_material' ? 'textbook_material' : 'instructor'
      const it = cat.data.amountItems.find((i) => i.code === code)
      if (it) setItemId(it.id)
    }
  }, [cat.data, itemId, prefill])

  const picks = useQuery({
    queryKey: ['entryPicks', monthId, programId, studentId],
    queryFn: () => api.entryPicks(monthId as number, programId as number, studentId === '' ? null : studentId),
    enabled: monthId !== '' && programId !== '' && kind !== 'fund_transfer',
  })
  const transfers = useQuery({
    queryKey: ['adjTransfers', yearId, monthId, programId],
    queryFn: () => api.adjustmentList({ schoolYearId: yearId, fiscalMonthId: monthId as number, programId: programId as number, kind: 'fund_transfer' }),
    enabled: kind === 'refund_return' && monthId !== '' && programId !== '',
  })

  const input: AdjustmentInput | null = useMemo(() => {
    if (monthId === '' || programId === '' || itemId === '') return null
    return {
      schoolYearId: yearId,
      fiscalMonthId: monthId,
      kind,
      programId,
      studentId: studentId === '' ? null : studentId,
      fromFundId: kind === 'fund_transfer' && fromFund !== '' ? fromFund : null,
      toFundId: kind === 'fund_transfer' && toFund !== '' ? toFund : null,
      fundTypeId: kind !== 'fund_transfer' && fund !== '' ? fund : null,
      amountItemId: itemId,
      amount: Math.trunc(amount),
      decidedOn: decidedOn || null,
      processedOn: processedOn || null,
      status,
      reason,
      relatedEntryId: relatedEntry === '' ? null : relatedEntry,
      relatedAdjustmentId: relatedAdj === '' ? null : relatedAdj,
      confirmDuplicate: confirmDup,
    }
  }, [yearId, monthId, programId, studentId, kind, fromFund, toFund, fund, itemId, amount, decidedOn, processedOn, status, reason, relatedEntry, relatedAdj, confirmDup])

  useEffect(() => {
    let alive = true
    if (!input) {
      setPreview(null)
      return
    }
    api
      .adjustmentPreview(input)
      .then((p) => alive && (setPreview(p), setPreviewErr(null)))
      .catch((e) => alive && (setPreview(null), setPreviewErr(errorMessage(e))))
    return () => {
      alive = false
    }
  }, [input])

  async function submit() {
    if (!input) return
    setBusy(true)
    try {
      const a = await api.adjustmentCreate(input)
      toast.show(`${a.kindLabel}을(를) 등록했습니다 (${a.statusLabel}).`)
      await qc.invalidateQueries()
      setConfirming(false)
      onDone()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  const funds = (cat.data?.fundTypes ?? []).filter((f) => f.isActive && f.mergedIntoId === null)
  const title = kind === 'fund_transfer' ? '재원경정 등록' : kind === 'refund' ? '수강료 환불 등록' : kind === 'refund_return' ? '재원경정에 따른 반환 이행 등록' : '추가징수 등록'

  return (
    <>
      <Card title={title}>
        {hintFunds.length > 0 && (
          <Notice>
            <div>
              <strong>차액 상세에서 가져온 재원별 차액</strong> — {hintFunds.map((f) => `${f.fundName} ${diff(f.diff ?? 0)}`).join(' · ')}. 경정 후보일 뿐이며 방향과 금액은 원본 회계자료로 확인한 뒤 직접 입력하세요.
            </div>
          </Notice>
        )}
        {baseKind === 'refund' && (
          <div className="row" style={{ marginBottom: 10 }}>
            <span className="label">환불 유형</span>
            <select className="select" value={kind} onChange={(e) => setKind(e.target.value as AdjustKind)}>
              <option value="refund">수강료 환불 (징수 기준액 감소)</option>
              <option value="refund_return">재원경정에 따른 반환 이행 (징수액 다시 차감하지 않음)</option>
            </select>
          </div>
        )}
        <div className="upload__top">
          <label className="field"><span>귀속월</span>
            <select className="select" value={monthId} onChange={(e) => setMonthId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">— 선택 —</option>
              {(months.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.label} ({m.ym}){m.status === 'closed' ? ' · 확정됨' : ''}</option>)}
            </select>
          </label>
          <label className="field"><span>프로그램</span>
            <select className="select" value={programId} onChange={(e) => setProgramId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">— 선택 —</option>
              {(programs.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </label>
          <label className="field"><span>학생 (없으면 프로그램 단위)</span>
            <select className="select" value={studentId} onChange={(e) => setStudentId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">— 프로그램 단위 —</option>
              {(students.data ?? []).map((s) => <option key={s.id} value={s.id}>{s.label}</option>)}
            </select>
          </label>
          <label className="field"><span>금액 항목</span>
            <select className="select" value={itemId} onChange={(e) => setItemId(e.target.value === '' ? '' : Number(e.target.value))}>
              {(cat.data?.amountItems ?? []).filter((i) => i.isActive).map((i) => <option key={i.id} value={i.id}>{i.name}</option>)}
            </select>
          </label>
          {kind === 'fund_transfer' ? (
            <>
              <label className="field"><span>변경 전 재원</span>
                <select className="select" value={fromFund} onChange={(e) => setFromFund(e.target.value === '' ? '' : Number(e.target.value))}>
                  <option value="">— 선택 —</option>
                  {funds.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
                </select>
              </label>
              <label className="field"><span>변경 후 재원</span>
                <select className="select" value={toFund} onChange={(e) => setToFund(e.target.value === '' ? '' : Number(e.target.value))}>
                  <option value="">— 선택 —</option>
                  {funds.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
                </select>
              </label>
            </>
          ) : kind === 'refund_return' ? (
            <label className="field"><span>근거 재원경정</span>
              <select className="select" value={relatedAdj} onChange={(e) => setRelatedAdj(e.target.value === '' ? '' : Number(e.target.value))}>
                <option value="">— 선택 —</option>
                {(transfers.data ?? []).filter((t) => t.status !== 'cancelled').map((t) => <option key={t.id} value={t.id}>#{t.id} {t.fromFundName} → {t.toFundName} {won(t.amount)} ({t.student || '프로그램 단위'})</option>)}
              </select>
            </label>
          ) : (
            <label className="field"><span>재원</span>
              <select className="select" value={fund} onChange={(e) => setFund(e.target.value === '' ? '' : Number(e.target.value))}>
                <option value="">— 선택 —</option>
                {funds.map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
              </select>
            </label>
          )}
          <label className="field"><span>금액 (원)</span>
            <input className="input" type="number" step={1} min={1} value={amount || ''} onChange={(e) => setAmount(Number(e.target.value))} />
          </label>
          <label className="field"><span>결정일</span>
            <input className="input" type="date" value={decidedOn} onChange={(e) => setDecidedOn(e.target.value)} />
          </label>
          <label className="field"><span>실제 처리일</span>
            <input className="input" type="date" value={processedOn} onChange={(e) => setProcessedOn(e.target.value)} />
          </label>
          <label className="field"><span>처리 상태</span>
            <select className="select" value={status} onChange={(e) => setStatus(e.target.value as 'planned' | 'done')}>
              <option value="done">처리 완료 (집계 반영)</option>
              <option value="planned">처리 예정 (집계 제외, 목록에만)</option>
            </select>
          </label>
          {kind !== 'fund_transfer' && kind !== 'refund_return' && (
            <label className="field"><span>관련 원본 거래 (선택)</span>
              <select className="select" value={relatedEntry} onChange={(e) => setRelatedEntry(e.target.value === '' ? '' : Number(e.target.value))}>
                <option value="">— 없음 —</option>
                {(picks.data ?? []).map((p) => <option key={p.id} value={p.id}>#{p.id} {p.kindLabel} {p.fundName} {p.itemName} {won(p.amount)} ({p.fileName})</option>)}
              </select>
            </label>
          )}
          <label className="field" style={{ gridColumn: 'span 2' }}><span>사유 (필수)</span>
            <input className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="예: 자유수강권 대상자 확정으로 재원 변경" />
          </label>
        </div>

        {previewErr && <Notice kind="bad">{previewErr}</Notice>}
        {preview && (
          <div style={{ marginTop: 12 }}>
            <div className="row">
              <div className="stat"><span>프로그램 · 귀속월</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{preview.programName} · {preview.monthLabel}</strong></div>
              <div className="stat"><span>학생</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{preview.student || '프로그램 단위'}</strong></div>
              {preview.refundable !== null && <div className="stat"><span>환불 가능 금액</span><strong>{won(preview.refundable)}</strong></div>}
              {kind === 'refund' && <div className="stat"><span>기존 환불액</span><strong>{won(preview.existingRefunds)}</strong></div>}
              <div className="stat"><span>전체 기준액 (전 → 후)</span><strong>{won(preview.totalBefore)} → {won(preview.totalAfter)}</strong></div>
            </div>
            {preview.funds.length > 0 && (
              <div className="tableWrap" style={{ marginTop: 8 }}>
                <table className="table">
                  <thead><tr><th>재원</th><th className="num">경정 전</th><th className="num">변동</th><th className="num">경정 후</th></tr></thead>
                  <tbody>
                    {preview.funds.map((f) => (
                      <tr key={f.fundId}><td>{f.fundName}</td><td className="num">{won(f.before)}</td><td className={`num ${f.change > 0 ? 'diffcell--up' : 'diffcell--down'}`}>{diff(f.change)}</td><td className="num">{won(f.after)}</td></tr>
                    ))}
                    <tr><td><strong>합계</strong></td><td className="num"><strong>{won(preview.totalBefore)}</strong></td><td className="num">{diff(preview.totalAfter - preview.totalBefore)}</td><td className="num"><strong>{won(preview.totalAfter)}</strong></td></tr>
                  </tbody>
                </table>
              </div>
            )}
            {preview.duplicates.length > 0 && (
              <div className="notice notice--soon" style={{ display: 'block', marginTop: 8 }}>
                <strong>같은 경제적 사건일 수 있는 기록</strong>
                <ul className="list">{preview.duplicates.map((d) => <li key={d}>{d}</li>)}</ul>
                <label className="row" style={{ marginTop: 4 }}><input type="checkbox" checked={confirmDup} onChange={(e) => setConfirmDup(e.target.checked)} /> 중복 아님 확인 — 실제로 별도 거래입니다</label>
              </div>
            )}
            {preview.warnings.map((w) => <div key={w} className="notice" style={{ marginTop: 6 }}>{w}</div>)}
            {preview.blocked.map((b) => <Notice key={b} kind="bad">{b}</Notice>)}
            <div className="row" style={{ marginTop: 10, justifyContent: 'flex-end' }}>
              <Button variant="primary" disabled={busy || preview.blocked.length > 0} onClick={() => setConfirming(true)}>등록…</Button>
            </div>
          </div>
        )}
      </Card>
      {confirming && preview && (
        <Modal title={`${preview.kindLabel} 등록 확인`} onClose={() => setConfirming(false)} footer={<><Button onClick={() => setConfirming(false)}>취소</Button><Button variant="primary" disabled={busy} onClick={submit}>등록</Button></>}>
          <p style={{ marginTop: 0 }}>{preview.programName} · {preview.monthLabel} · {preview.student || '프로그램 단위'} · {won(amount)}원 · {status === 'done' ? '처리 완료' : '처리 예정'}{processedOn ? ` · 처리일 ${processedOn}` : ''}</p>
          <p className="muted" style={{ marginBottom: 0 }}>전체 기준액 {won(preview.totalBefore)} → {won(preview.totalAfter)}. 등록 뒤 월별 정산과 차액 분석이 다시 계산되며, 기존 확인 메모가 있으면 '재확인 필요'로 표시됩니다.</p>
        </Modal>
      )}
    </>
  )
}

/* ── 정정 이력 ───────────────────────────────────────────────────── */

const STATUS_CLS: Record<string, string> = { planned: 'badge badge--soon', done: 'badge badge--ok', cancelled: 'badge badge--muted' }

function History({ yearId }: { yearId: number }) {
  const qc = useQueryClient()
  const toast = useToast()
  const months = useQuery({ queryKey: ['months', yearId], queryFn: () => api.monthList(yearId) })
  const programs = useQuery({ queryKey: ['programs', yearId], queryFn: () => api.programList(yearId) })
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })
  const [monthId, setMonthId] = useState<number | ''>('')
  const [from, setFrom] = useState('')
  const [to, setTo] = useState('')
  const [programId, setProgramId] = useState<number | ''>('')
  const [studentText, setStudentText] = useState('')
  const [fundId, setFundId] = useState<number | ''>('')
  const [kind, setKind] = useState('')
  const [status, setStatus] = useState('')
  const [doneFor, setDoneFor] = useState<Adjustment | null>(null)
  const [cancelFor, setCancelFor] = useState<Adjustment | null>(null)
  const [date, setDate] = useState(today())
  const [reason, setReason] = useState('')

  const list = useQuery({
    queryKey: ['adjustments', yearId, monthId, from, to, programId, studentText, fundId, kind, status],
    queryFn: () => api.adjustmentList({ schoolYearId: yearId, fiscalMonthId: monthId === '' ? null : monthId, processedFrom: from || null, processedTo: to || null, programId: programId === '' ? null : programId, studentText: studentText || null, fundTypeId: fundId === '' ? null : fundId, kind: kind || null, status: status || null }),
  })

  async function run(f: () => Promise<unknown>, msg: string) {
    try {
      await f()
      toast.show(msg)
      setDoneFor(null)
      setCancelFor(null)
      setReason('')
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }

  return (
    <>
      <Card title="조회 조건 (귀속월 기준 또는 처리일 기간 기준)">
        <div className="row">
          <label className="field"><span>귀속월</span>
            <select className="select" value={monthId} onChange={(e) => setMonthId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">전체</option>
              {(months.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.label} ({m.ym})</option>)}
            </select>
          </label>
          <label className="field"><span>처리일 (부터)</span><input className="input" type="date" value={from} onChange={(e) => setFrom(e.target.value)} /></label>
          <label className="field"><span>처리일 (까지)</span><input className="input" type="date" value={to} onChange={(e) => setTo(e.target.value)} /></label>
          <label className="field"><span>프로그램</span>
            <select className="select" value={programId} onChange={(e) => setProgramId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">전체</option>
              {(programs.data ?? []).map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
            </select>
          </label>
          <label className="field"><span>학생 이름</span><input className="input" value={studentText} onChange={(e) => setStudentText(e.target.value)} style={{ width: 120 }} /></label>
          <label className="field"><span>재원</span>
            <select className="select" value={fundId} onChange={(e) => setFundId(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">전체</option>
              {(cat.data?.fundTypes ?? []).map((f) => <option key={f.id} value={f.id}>{f.name}</option>)}
            </select>
          </label>
          <label className="field"><span>거래 유형</span>
            <select className="select" value={kind} onChange={(e) => setKind(e.target.value)}>
              <option value="">전체</option><option value="fund_transfer">재원경정</option><option value="refund">수강료 환불</option><option value="refund_return">경정 반환 이행</option><option value="extra_collect">추가징수</option>
            </select>
          </label>
          <label className="field"><span>처리 상태</span>
            <select className="select" value={status} onChange={(e) => setStatus(e.target.value)}>
              <option value="">전체</option><option value="planned">처리 예정</option><option value="done">처리 완료</option><option value="cancelled">취소</option>
            </select>
          </label>
        </div>
      </Card>
      <Card title="정정 이력">
        {list.error && <Notice kind="bad">{errorMessage(list.error)}</Notice>}
        {list.data && list.data.length === 0 && <Empty title="조건에 맞는 정정이 없습니다." />}
        {list.data && list.data.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>등록</th><th>유형</th><th>귀속월</th><th>처리일</th><th>프로그램</th><th>학생</th><th>재원</th><th>항목</th><th className="num">금액</th><th className="center">상태</th><th>사유 · 연결</th><th />
                </tr>
              </thead>
              <tbody>
                {list.data.map((a) => (
                  <tr key={a.id} className={a.status === 'cancelled' ? 'row--off' : a.orphan ? 'row--error' : ''}>
                    <td className="muted">{shortDateTime(a.createdAt)}</td>
                    <td>{a.kindLabel}</td>
                    <td>{a.monthLabel}</td>
                    <td>{a.processedOn ?? <span className="muted">—</span>}</td>
                    <td>{a.programName}</td>
                    <td>{a.student || <span className="muted">프로그램 단위</span>}</td>
                    <td>{a.kind === 'fund_transfer' ? `${a.fromFundName} → ${a.toFundName}` : a.fundName}</td>
                    <td>{a.itemName}</td>
                    <td className="num">{won(a.amount)}</td>
                    <td className="center">
                      <span className={STATUS_CLS[a.status]}>{a.statusLabel}</span>
                      {a.orphan && <span className="badge badge--bad" style={{ marginLeft: 4 }}>원본 재확인 필요</span>}
                    </td>
                    <td className="wrap" style={{ fontSize: 12 }}>
                      {a.reason}
                      {a.relatedEntryId !== null && <span className="muted"> · 원본 거래 #{a.relatedEntryId}{a.relatedEntryState !== 'active' ? ' (취소됨)' : ''}</span>}
                      {a.relatedAdjustmentId !== null && <span className="muted"> · 근거 경정 #{a.relatedAdjustmentId}</span>}
                      {a.entryIds.length > 0 && <span className="muted"> · 원장 거래 {a.entryIds.map((e) => `#${e}`).join(', ')}</span>}
                      {a.status === 'cancelled' && <span className="muted"> · 취소: {a.cancelReason}</span>}
                    </td>
                    <td>
                      <div className="row" style={{ gap: 4, flexWrap: 'nowrap' }}>
                        {a.status === 'planned' && <Button size="sm" onClick={() => setDoneFor(a)}>완료 처리…</Button>}
                        {a.status !== 'cancelled' && <Button size="sm" onClick={() => setCancelFor(a)}>취소…</Button>}
                      </div>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {doneFor && (
        <Modal title={`${doneFor.kindLabel} #${doneFor.id} 처리 완료`} onClose={() => setDoneFor(null)} footer={<><Button onClick={() => setDoneFor(null)}>닫기</Button><Button variant="primary" disabled={!date} onClick={() => run(() => api.adjustmentSetStatus(doneFor.id, 'done', date, reason), '처리 완료로 바꿨습니다.')}>완료</Button></>}>
          <div className="row"><span className="label">실제 처리일</span><input className="input" type="date" value={date} onChange={(e) => setDate(e.target.value)} /></div>
          <div className="row" style={{ marginTop: 8 }}><span className="label">메모</span><input className="input" style={{ flex: 1 }} value={reason} onChange={(e) => setReason(e.target.value)} /></div>
          <p className="muted">완료되면 원장 거래가 실제 처리로 바뀌어 집계에 반영됩니다. 처리 예정으로 되돌릴 수는 없고 취소만 됩니다.</p>
        </Modal>
      )}
      {cancelFor && (
        <Modal title={`${cancelFor.kindLabel} #${cancelFor.id} 취소`} onClose={() => setCancelFor(null)} footer={<><Button onClick={() => setCancelFor(null)}>닫기</Button><Button variant="primary" disabled={!reason.trim()} onClick={() => run(() => api.adjustmentCancel(cancelFor.id, reason), '취소했습니다.')}>취소 실행</Button></>}>
          <div className="row"><span className="label">취소 사유</span>
            <select className="select" value={reason} onChange={(e) => setReason(e.target.value)} style={{ flex: 1 }}>
              <option value="">— 선택 —</option>
              {['학생 선택 오류', '금액 입력 오류', '재원 선택 오류', '중복 등록', '실제 회계 처리 취소'].map((r) => <option key={r} value={r}>{r}</option>)}
            </select>
          </div>
          <p className="muted">원장 거래는 취소 표시만 되고 기록은 남습니다. 재원경정은 두 거래가 함께 취소됩니다.</p>
        </Modal>
      )}
    </>
  )
}
