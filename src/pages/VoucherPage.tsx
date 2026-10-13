import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { diff, shortDateTime, won } from '@/lib/format'
import { useApp } from '@/lib/useApp'
import type {
  VoucherBasis,
  VoucherDecision,
  VoucherException,
  VoucherPolicy,
  VoucherProposal,
  VoucherRefundRow,
  VoucherRetroPreview,
  VoucherTarget,
  VoucherTargetStatus,
  VoucherUsage,
} from '@/ipc/types'

type Tab = 'targets' | 'policy' | 'usage' | 'retro' | 'refund' | 'transfer' | 'history'

const TABS: [Tab, string][] = [
  ['targets', '대상자 관리'],
  ['policy', '지원 한도 설정'],
  ['usage', '사용액 현황'],
  ['retro', '소급 지원 계산'],
  ['refund', '환불 처리'],
  ['transfer', '전입생 관리'],
  ['history', '처리 이력'],
]

const today = () => new Date().toISOString().slice(0, 10)

const TARGET_CLS: Record<string, string> = { pending: 'badge badge--soon', confirmed: 'badge badge--ok', ended: 'badge badge--muted', cancelled: 'badge badge--muted' }
const USAGE_CLS: Record<string, string> = { ok: 'badge badge--ok', retro: 'badge badge--navy', over: 'badge badge--bad', decide: 'badge badge--soon', pending: 'badge badge--soon', outside: 'badge badge--muted' }
const PROPOSAL_CLS: Record<string, string> = { normal: 'badge badge--ok', blocked: 'badge badge--bad', decide: 'badge badge--soon', pending: 'badge badge--soon', over: 'badge badge--bad', short: 'badge badge--muted' }
const REFUND_CLS: Record<string, string> = { none: 'badge badge--bad', planned: 'badge badge--soon', partial: 'badge badge--navy', complete: 'badge badge--ok', cancelled: 'badge badge--muted' }

export function VoucherPage() {
  const { year } = useApp()
  const [tab, setTab] = useState<Tab>('targets')
  if (!year) {
    return (
      <Page title="자유수강권 관리">
        <Notice>학년도가 없습니다.</Notice>
      </Page>
    )
  }
  return (
    <Page title="자유수강권 관리" sub="대상자·지원 한도·사용액을 관리하고, 소급 지원 경정과 환불을 회계 정정(재원경정·반환 이행)으로 등록합니다. 지원액은 자동으로 확정되지 않고, 담당자가 미리보기를 확인한 뒤 승인한 묶음만 등록됩니다.">
      <div className="tabs">
        {TABS.map(([k, l]) => (
          <button key={k} type="button" className={tab === k ? 'tab tab--on' : 'tab'} onClick={() => setTab(k)}>
            {l}
          </button>
        ))}
      </div>
      {tab === 'targets' && <Targets yearId={year.id} />}
      {tab === 'policy' && <Policy yearId={year.id} />}
      {tab === 'usage' && <Usage yearId={year.id} />}
      {tab === 'retro' && <Retro yearId={year.id} />}
      {tab === 'refund' && <Refund yearId={year.id} />}
      {tab === 'transfer' && <TransferIn yearId={year.id} />}
      {tab === 'history' && <History yearId={year.id} />}
    </Page>
  )
}

function useRun() {
  const toast = useToast()
  const qc = useQueryClient()
  return async (f: () => Promise<unknown>, msg: string, after?: () => void) => {
    try {
      await f()
      toast.show(msg)
      await qc.invalidateQueries()
      after?.()
      return true
    } catch (e) {
      toast.show(errorMessage(e), true)
      return false
    }
  }
}

/* ── 대상자 관리 ─────────────────────────────────────────────── */

function Targets({ yearId }: { yearId: number }) {
  const run = useRun()
  const targets = useQuery({ queryKey: ['voucherTargets', yearId], queryFn: () => api.voucherTargetList(yearId) })
  const students = useQuery({ queryKey: ['students', yearId], queryFn: () => api.studentList(yearId) })
  const [editing, setEditing] = useState<VoucherTarget | 'new' | null>(null)
  const [open, setOpen] = useState<number | null>(null)
  const [filter, setFilter] = useState('all')

  const rows = (targets.data ?? []).filter((t) => (filter === 'all' ? true : t.status === filter))
  return (
    <>
      <Card title="대상자 목록" right={<Button variant="primary" onClick={() => setEditing('new')}>대상자 등록…</Button>}>
        <div className="row" style={{ marginBottom: 8 }}>
          <label className="field"><span>상태</span>
            <select className="select" value={filter} onChange={(e) => setFilter(e.target.value)}>
              <option value="all">전체</option><option value="pending">확인 전</option><option value="confirmed">대상자 확정</option><option value="ended">지원 종료</option><option value="cancelled">대상 취소</option>
            </select>
          </label>
          <span className="muted" style={{ alignSelf: 'flex-end' }}>대상자 파일 업로드는 실제 양식이 확인되기 전까지 제공하지 않습니다. 학생을 골라 직접 등록합니다.</span>
        </div>
        {targets.error && <Notice kind="bad">{errorMessage(targets.error)}</Notice>}
        {targets.data && rows.length === 0 && <Empty title="등록된 대상자가 없습니다." />}
        {rows.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>학생</th><th className="center">상태</th><th>확정일</th><th>적용 시작</th><th>적용 종료</th><th>전입</th><th>이전 학교 사용액</th><th>예외 한도</th><th>메모</th><th /></tr></thead>
              <tbody>
                {rows.map((t) => (
                  <>
                    <tr key={t.id} className={t.status === 'cancelled' ? 'row--off' : ''}>
                      <td>{t.student}</td>
                      <td className="center"><span className={TARGET_CLS[t.status]}>{t.statusLabel}</span></td>
                      <td>{t.confirmedOn ?? <span className="muted">—</span>}</td>
                      <td>{t.applyFrom ?? <span className="muted">—</span>}</td>
                      <td>{t.applyTo ?? <span className="muted">계속</span>}</td>
                      <td>{t.transferIn ? `전입 ${t.transferInOn ?? ''}` : <span className="muted">—</span>}</td>
                      <td>{t.transferIn ? (t.priorNeedsConfirm ? <span className="badge badge--bad">{t.priorStatusLabel}</span> : `${won(t.priorUsed)}원 확인됨`) : <span className="muted">—</span>}</td>
                      <td>{t.hasException ? <span className="badge badge--navy">예외</span> : <span className="muted">정책</span>}</td>
                      <td className="wrap" style={{ fontSize: 12 }}>{t.note}</td>
                      <td>
                        <div className="row" style={{ gap: 4, flexWrap: 'nowrap' }}>
                          <Button size="sm" onClick={() => setEditing(t)}>수정…</Button>
                          <Button size="sm" onClick={() => setOpen(open === t.id ? null : t.id)}>이력 {t.logs.length}</Button>
                        </div>
                      </td>
                    </tr>
                    {open === t.id && (
                      <tr key={`${t.id}-log`}>
                        <td colSpan={10} style={{ background: 'var(--bg-sub, #f7f7f9)' }}>
                          <ul className="list" style={{ margin: 0 }}>
                            {t.logs.map((l, i) => <li key={i}><span className="muted">{shortDateTime(l.at)}</span> · {LOG_ACTION[l.action] ?? l.action}{l.fromStatus || l.toStatus ? ` (${labelOf(l.fromStatus)} → ${labelOf(l.toStatus)})` : ''} {l.detail}</li>)}
                          </ul>
                        </td>
                      </tr>
                    )}
                  </>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {editing && <TargetForm yearId={yearId} target={editing === 'new' ? null : editing} students={students.data ?? []} onClose={() => setEditing(null)} run={run} />}
    </>
  )
}

const LOG_ACTION: Record<string, string> = { register: '등록', status: '상태 변경', dates: '날짜·정보 변경', prior: '이전 학교 사용액', note: '메모' }
const STATUS_LABEL: Record<string, string> = { pending: '확인 전', confirmed: '대상자 확정', ended: '지원 종료', cancelled: '대상 취소', unconfirmed: '확인 필요', '': '—' }
const labelOf = (s: string) => STATUS_LABEL[s] ?? s

function TargetForm({ yearId, target, students, onClose, run }: { yearId: number; target: VoucherTarget | null; students: { id: number; label: string }[]; onClose: () => void; run: ReturnType<typeof useRun> }) {
  const [studentId, setStudentId] = useState<number | ''>(target?.studentId ?? '')
  const [status, setStatus] = useState<VoucherTargetStatus>(target?.status ?? 'pending')
  const [confirmedOn, setConfirmedOn] = useState(target?.confirmedOn ?? '')
  const [applyFrom, setApplyFrom] = useState(target?.applyFrom ?? '')
  const [applyTo, setApplyTo] = useState(target?.applyTo ?? '')
  const [transferIn, setTransferIn] = useState(target?.transferIn ?? false)
  const [transferInOn, setTransferInOn] = useState(target?.transferInOn ?? '')
  const [note, setNote] = useState(target?.note ?? '')
  const [reason, setReason] = useState('')
  const [newStudent, setNewStudent] = useState(false)
  const [grade, setGrade] = useState(1)
  const [className, setClassName] = useState('')
  const [studentNo, setStudentNo] = useState('')
  const [name, setName] = useState('')
  const toast = useToast()
  const qc = useQueryClient()

  async function createStudent() {
    try {
      const s = await api.studentCreate({ schoolYearId: yearId, grade, className, studentNo, name })
      await qc.invalidateQueries({ queryKey: ['students', yearId] })
      setStudentId(s.id)
      setNewStudent(false)
      toast.show('학생을 등록했습니다.')
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }
  function submit() {
    if (studentId === '') return
    run(
      () => api.voucherTargetSave({ schoolYearId: yearId, studentId, status, confirmedOn: confirmedOn || null, applyFrom: applyFrom || null, applyTo: applyTo || null, transferIn, transferInOn: transferIn ? transferInOn || null : null, note, reason }),
      '대상자를 저장했습니다.',
      onClose,
    )
  }
  return (
    <Modal title={target ? `대상자 수정 — ${target.student}` : '대상자 등록'} onClose={onClose} width={720} footer={<><Button onClick={onClose}>닫기</Button><Button variant="primary" disabled={studentId === ''} onClick={submit}>저장</Button></>}>
      <div className="upload__top">
        {!target && (
          <label className="field" style={{ gridColumn: 'span 2' }}><span>학생</span>
            <div className="row">
              <select className="select" style={{ flex: 1 }} value={studentId} onChange={(e) => setStudentId(e.target.value === '' ? '' : Number(e.target.value))}>
                <option value="">— 선택 —</option>
                {students.map((s) => <option key={s.id} value={s.id}>{s.label}</option>)}
              </select>
              <Button size="sm" onClick={() => setNewStudent(!newStudent)}>명단에 없는 학생…</Button>
            </div>
          </label>
        )}
        {newStudent && (
          <div className="notice" style={{ gridColumn: 'span 2', display: 'block' }}>
            <div className="row">
              <label className="field"><span>학년</span><input className="input" type="number" min={1} max={6} value={grade} onChange={(e) => setGrade(Number(e.target.value))} style={{ width: 60 }} /></label>
              <label className="field"><span>반</span><input className="input" value={className} onChange={(e) => setClassName(e.target.value)} style={{ width: 80 }} /></label>
              <label className="field"><span>번호</span><input className="input" value={studentNo} onChange={(e) => setStudentNo(e.target.value)} style={{ width: 60 }} /></label>
              <label className="field"><span>이름</span><input className="input" value={name} onChange={(e) => setName(e.target.value)} style={{ width: 120 }} /></label>
              <Button size="sm" variant="primary" onClick={createStudent} disabled={!name.trim()}>학생 등록</Button>
            </div>
            <p className="muted" style={{ margin: '6px 0 0' }}>학년·반·번호·이름이 모두 같은 학생이 이미 있으면 그 학생을 씁니다.</p>
          </div>
        )}
        <label className="field"><span>상태</span>
          <select className="select" value={status} onChange={(e) => setStatus(e.target.value as VoucherTargetStatus)}>
            <option value="pending">확인 전</option><option value="confirmed">대상자 확정</option><option value="ended">지원 종료</option><option value="cancelled">대상 취소</option>
          </select>
        </label>
        <label className="field"><span>확정일</span><input className="input" type="date" value={confirmedOn} onChange={(e) => setConfirmedOn(e.target.value)} /></label>
        <label className="field"><span>적용 시작일 (소급 기준)</span><input className="input" type="date" value={applyFrom} onChange={(e) => setApplyFrom(e.target.value)} /></label>
        <label className="field"><span>적용 종료일 (지원 종료 때)</span><input className="input" type="date" value={applyTo} onChange={(e) => setApplyTo(e.target.value)} /></label>
        <label className="field"><span>전입생</span>
          <label className="row" style={{ height: 'var(--h-control, 28px)' }}><input type="checkbox" checked={transferIn} onChange={(e) => setTransferIn(e.target.checked)} /> 전입생 (이전 학교 사용액 확인 필요)</label>
        </label>
        <label className="field"><span>전입일</span><input className="input" type="date" value={transferInOn} disabled={!transferIn} onChange={(e) => setTransferInOn(e.target.value)} /></label>
        <label className="field" style={{ gridColumn: 'span 2' }}><span>메모</span><input className="input" value={note} onChange={(e) => setNote(e.target.value)} /></label>
        {target && target.status !== status && (
          <label className="field" style={{ gridColumn: 'span 2' }}><span>상태 변경 사유 (필수)</span><input className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="예: 교육청 대상자 통보 확인" /></label>
        )}
      </div>
      <p className="muted" style={{ marginBottom: 0 }}>저장해도 지원액이 확정되지 않습니다. 적용 시작일 이후 달의 사용액은 '소급 지원 계산'에서 미리보기로 확인한 뒤 담당자가 승인해야 경정됩니다.</p>
    </Modal>
  )
}

/* ── 지원 한도 설정 ─────────────────────────────────────────── */

function PolicyFields({ basis, setBasis, annual, setAnnual, s1, setS1, s2, setS2, carry, setCarry }: { basis: VoucherBasis; setBasis: (b: VoucherBasis) => void; annual: number; setAnnual: (n: number) => void; s1: number; setS1: (n: number) => void; s2: number; setS2: (n: number) => void; carry: boolean; setCarry: (b: boolean) => void }) {
  return (
    <>
      <label className="field"><span>한도 기준</span>
        <select className="select" value={basis} onChange={(e) => setBasis(e.target.value as VoucherBasis)}>
          <option value="annual">연간 한도</option><option value="semester">학기별 한도</option><option value="unlimited">제한 없음</option>
        </select>
      </label>
      {basis === 'annual' && <label className="field"><span>연간 한도 (원)</span><input className="input" type="number" min={0} step={1} value={annual || ''} onChange={(e) => setAnnual(Number(e.target.value))} /></label>}
      {basis === 'semester' && (
        <>
          <label className="field"><span>1학기 한도 (3~8월, 원)</span><input className="input" type="number" min={0} step={1} value={s1 || ''} onChange={(e) => setS1(Number(e.target.value))} /></label>
          <label className="field"><span>2학기 한도 (9~2월, 원)</span><input className="input" type="number" min={0} step={1} value={s2 || ''} onChange={(e) => setS2(Number(e.target.value))} /></label>
          <label className="field"><span>잔액 이월</span>
            <label className="row" style={{ height: 'var(--h-control, 28px)' }}><input type="checkbox" checked={carry} onChange={(e) => setCarry(e.target.checked)} /> 1학기 잔액을 2학기로 이월</label>
          </label>
        </>
      )}
    </>
  )
}

function Policy({ yearId }: { yearId: number }) {
  const run = useRun()
  const current = useQuery({ queryKey: ['voucherPolicy', yearId], queryFn: () => api.voucherPolicyCurrent(yearId) })
  const history = useQuery({ queryKey: ['voucherPolicyHistory', yearId], queryFn: () => api.voucherPolicyHistory(yearId) })
  const exceptions = useQuery({ queryKey: ['voucherExceptions', yearId], queryFn: () => api.voucherExceptionList(yearId, false) })
  const students = useQuery({ queryKey: ['students', yearId], queryFn: () => api.studentList(yearId) })
  const [basis, setBasis] = useState<VoucherBasis>('annual')
  const [annual, setAnnual] = useState(0)
  const [s1, setS1] = useState(0)
  const [s2, setS2] = useState(0)
  const [carry, setCarry] = useState(false)
  const [note, setNote] = useState('')
  const [confirming, setConfirming] = useState(false)
  const [xStudent, setXStudent] = useState<number | ''>('')
  const [xBasis, setXBasis] = useState<VoucherBasis>('annual')
  const [xAnnual, setXAnnual] = useState(0)
  const [xS1, setXS1] = useState(0)
  const [xS2, setXS2] = useState(0)
  const [xCarry, setXCarry] = useState(false)
  const [xReason, setXReason] = useState('')
  const [ending, setEnding] = useState<VoucherException | null>(null)
  const [endReason, setEndReason] = useState('')

  useEffect(() => {
    const p = current.data
    if (!p) return
    setBasis(p.basis)
    setAnnual(p.annualLimit ?? 0)
    setS1(p.semester1Limit ?? 0)
    setS2(p.semester2Limit ?? 0)
    setCarry(p.carryOver)
  }, [current.data])

  const policyLabel = (p: VoucherPolicy | VoucherException) => (p.basis === 'annual' ? `연간 ${won(p.annualLimit)}원` : p.basis === 'semester' ? `1학기 ${won(p.semester1Limit)}원 · 2학기 ${won(p.semester2Limit)}원 · ${p.carryOver ? '잔액 이월' : '이월 없음'}` : '제한 없음')

  return (
    <>
      <Card title="학년도 지원 한도 정책" right={current.data ? <span className="muted">현재: {policyLabel(current.data)} (등록 {shortDateTime(current.data.createdAt)})</span> : <span className="muted">아직 정책이 없습니다</span>}>
        <div className="upload__top">
          <PolicyFields basis={basis} setBasis={setBasis} annual={annual} setAnnual={setAnnual} s1={s1} setS1={setS1} s2={s2} setS2={setS2} carry={carry} setCarry={setCarry} />
          <label className="field" style={{ gridColumn: 'span 2' }}><span>메모</span><input className="input" value={note} onChange={(e) => setNote(e.target.value)} placeholder="예: 2026학년도 교육청 지침" /></label>
        </div>
        <div className="row" style={{ marginTop: 10, justifyContent: 'flex-end' }}>
          <Button variant="primary" onClick={() => setConfirming(true)}>{current.data ? '정책 변경…' : '정책 등록…'}</Button>
        </div>
        <p className="muted" style={{ marginBottom: 0 }}>정책을 바꾸면 이전 정책은 이력으로 남고, 이후 사용액 계산은 새 정책으로 다시 계산됩니다. 이미 등록된 경정·환불은 바뀌지 않습니다.</p>
      </Card>
      <Card title="학생별 예외 한도">
        <div className="upload__top">
          <label className="field"><span>학생</span>
            <select className="select" value={xStudent} onChange={(e) => setXStudent(e.target.value === '' ? '' : Number(e.target.value))}>
              <option value="">— 선택 —</option>
              {(students.data ?? []).map((s) => <option key={s.id} value={s.id}>{s.label}</option>)}
            </select>
          </label>
          <PolicyFields basis={xBasis} setBasis={setXBasis} annual={xAnnual} setAnnual={setXAnnual} s1={xS1} setS1={setXS1} s2={xS2} setS2={setXS2} carry={xCarry} setCarry={setXCarry} />
          <label className="field" style={{ gridColumn: 'span 2' }}><span>예외 사유 (필수)</span><input className="input" value={xReason} onChange={(e) => setXReason(e.target.value)} /></label>
        </div>
        <div className="row" style={{ marginTop: 10, justifyContent: 'flex-end' }}>
          <Button variant="primary" disabled={xStudent === '' || !xReason.trim()} onClick={() => run(() => api.voucherExceptionSave({ schoolYearId: yearId, studentId: xStudent as number, basis: xBasis, annualLimit: xBasis === 'annual' ? xAnnual : null, semester1Limit: xBasis === 'semester' ? xS1 : null, semester2Limit: xBasis === 'semester' ? xS2 : null, carryOver: xCarry, reason: xReason }), '예외 한도를 저장했습니다.', () => setXReason(''))}>예외 저장</Button>
        </div>
        {(exceptions.data ?? []).length > 0 && (
          <div className="tableWrap" style={{ marginTop: 8 }}>
            <table className="table">
              <thead><tr><th>학생</th><th>한도</th><th>사유</th><th>등록</th><th /></tr></thead>
              <tbody>
                {(exceptions.data ?? []).map((x) => (
                  <tr key={x.id}><td>{x.student}</td><td>{policyLabel(x)}</td><td className="wrap">{x.reason}</td><td className="muted">{shortDateTime(x.createdAt)}</td><td><Button size="sm" onClick={() => setEnding(x)}>해제…</Button></td></tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      <Card title="정책 변경 이력">
        {(history.data ?? []).length === 0 && <Empty title="이력이 없습니다." />}
        {(history.data ?? []).length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>등록</th><th>한도</th><th>메모</th><th className="center">상태</th></tr></thead>
              <tbody>
                {(history.data ?? []).map((p) => (
                  <tr key={p.id} className={p.supersededAt ? 'row--off' : ''}><td className="muted">{shortDateTime(p.createdAt)}</td><td>{policyLabel(p)}</td><td className="wrap">{p.note}</td><td className="center">{p.supersededAt ? <span className="badge badge--muted">대체됨 {shortDateTime(p.supersededAt)}</span> : <span className="badge badge--ok">현재</span>}</td></tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {confirming && (
        <Modal title="지원 한도 정책 저장" onClose={() => setConfirming(false)} footer={<><Button onClick={() => setConfirming(false)}>취소</Button><Button variant="primary" onClick={() => run(() => api.voucherPolicySave({ schoolYearId: yearId, basis, annualLimit: basis === 'annual' ? annual : null, semester1Limit: basis === 'semester' ? s1 : null, semester2Limit: basis === 'semester' ? s2 : null, carryOver: carry, note }), '정책을 저장했습니다.', () => setConfirming(false))}>저장</Button></>}>
          <p>{basis === 'annual' ? `연간 ${won(annual)}원` : basis === 'semester' ? `1학기 ${won(s1)}원 · 2학기 ${won(s2)}원 · ${carry ? '잔액 이월' : '이월 없음'}` : '제한 없음'} 으로 저장합니다. 이전 정책은 이력으로 남습니다.</p>
        </Modal>
      )}
      {ending && (
        <Modal title={`예외 해제 — ${ending.student}`} onClose={() => setEnding(null)} footer={<><Button onClick={() => setEnding(null)}>닫기</Button><Button variant="primary" disabled={!endReason.trim()} onClick={() => run(() => api.voucherExceptionEnd(ending.id, endReason), '예외를 해제했습니다.', () => { setEnding(null); setEndReason('') })}>해제</Button></>}>
          <div className="row"><span className="label">해제 사유</span><input className="input" style={{ flex: 1 }} value={endReason} onChange={(e) => setEndReason(e.target.value)} /></div>
          <p className="muted">해제 뒤에는 학년도 정책이 적용됩니다. 예외 기록은 이력에 남습니다.</p>
        </Modal>
      )}
    </>
  )
}

/* ── 사용액 현황 ─────────────────────────────────────────────── */

function UsageSummary({ u }: { u: VoucherUsage }) {
  return (
    <div className="row">
      <div className="stat"><span>대상자 상태</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{u.targetStatusLabel}{u.applyFrom ? ` · ${u.applyFrom} ~ ${u.applyTo ?? '계속'}` : ''}</strong></div>
      <div className="stat"><span>한도 ({u.limitSource || '—'})</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{u.limitLabel || '—'}</strong></div>
      {u.priorUsed !== null && <div className="stat"><span>이전 학교 사용액</span><strong>{won(u.priorUsed)}</strong></div>}
      <div className="stat"><span>계산 지원액 (적용 기간)</span><strong>{won(u.usedExpected)}</strong></div>
      <div className="stat"><span>실제 자유수강권 사용액</span><strong>{won(u.usedActual)}</strong></div>
      {u.pendingTotal > 0 && <div className="stat stat--warn"><span>처리 예정 경정</span><strong>{won(u.pendingTotal)}</strong></div>}
      <div className="stat"><span>남은 한도</span><strong>{u.remaining === null ? (u.basis === 'unlimited' ? '제한 없음' : '—') : won(u.remaining)}</strong></div>
      {u.remainingS1 !== null && <div className="stat"><span>1학기 잔액 / 2학기 잔액</span><strong>{won(u.remainingS1)} / {won(u.remainingS2)}</strong></div>}
      {u.exhaustedYm && <div className="stat stat--warn"><span>한도 소진</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{u.exhaustedYm} 부터 수익자 전환</strong></div>}
      {u.retroTotal > 0 && <div className="stat stat--warn"><span>경정 필요 합계</span><strong>{won(u.retroTotal)}</strong></div>}
      {u.overTotal > 0 && <div className="stat stat--bad"><span>계산보다 많이 사용 (확인 대상)</span><strong>{won(u.overTotal)}</strong></div>}
    </div>
  )
}

function UsageRows({ u }: { u: VoucherUsage }) {
  if (u.rows.length === 0) return <Empty title="이 학생의 징수 거래가 없습니다." />
  return (
    <div className="tableWrap" style={{ marginTop: 8 }}>
      <table className="table">
        <thead><tr><th>귀속월</th><th>프로그램</th><th>항목</th><th className="num">수강료</th><th className="num">자유수강권 (실제)</th><th className="num">수익자 (실제)</th><th className="num">기타 재원</th><th className="num">계산 지원액</th><th className="num">계산 수익자</th><th className="num">차이</th><th className="num">남은 한도 (후)</th><th className="center">상태</th></tr></thead>
        <tbody>
          {u.rows.map((r) => (
            <tr key={`${r.fiscalMonthId}-${r.programId}-${r.amountItemId}`} className={r.state === 'over' ? 'row--error' : r.state === 'retro' || r.state === 'decide' ? 'row--check' : !r.inWindow ? 'row--off' : ''}>
              <td>{r.monthLabel}{r.monthClosed ? ' · 확정' : ''}</td>
              <td>{r.programName}</td>
              <td>{r.itemName}</td>
              <td className="num">{won(r.tuition)}</td>
              <td className="num">{won(r.freeActual)}</td>
              <td className="num">{won(r.beneficiaryActual)}</td>
              <td className="num" title={r.funds.filter((f) => f.fundName !== '자유수강권').map((f) => `${f.fundName} ${won(f.amount)}`).join(' · ')}>{won(r.otherActual)}</td>
              <td className="num">{r.inWindow ? won(r.freeExpected) : <span className="muted">—</span>}</td>
              <td className="num">{r.inWindow ? won(r.beneficiaryExpected) : <span className="muted">—</span>}</td>
              <td className={`num ${r.delta > 0 ? 'diffcell--up' : r.delta < 0 ? 'diffcell--down' : ''}`}>{diff(r.delta)}{r.pendingTransfer > 0 ? <span className="muted"> (예정 {won(r.pendingTransfer)})</span> : null}</td>
              <td className="num">{r.remainingAfter === null ? <span className="muted">—</span> : won(r.remainingAfter)}</td>
              <td className="center"><span className={USAGE_CLS[r.state]}>{r.stateLabel}</span></td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function UsageMessages({ u }: { u: VoucherUsage }) {
  return (
    <>
      {u.blocked.map((b) => <Notice key={b} kind="bad">{b}</Notice>)}
      {u.warnings.map((w) => <div key={w} className="notice notice--soon" style={{ marginTop: 6 }}>{w}</div>)}
      {u.notes.map((n) => <div key={n} className="notice" style={{ marginTop: 6 }}>{n}</div>)}
    </>
  )
}

function Usage({ yearId }: { yearId: number }) {
  const all = useQuery({ queryKey: ['voucherUsageAll', yearId], queryFn: () => api.voucherUsageAll(yearId) })
  const [picked, setPicked] = useState<number | 'all'>('all')
  const list = all.data ?? []
  const one = picked === 'all' ? null : list.find((u) => u.studentId === picked) ?? null
  return (
    <>
      <Card title="사용액 현황" right={<span className="muted">계산 지원액과 실제 자유수강권 사용액의 차이는 오류로 단정하지 않고 '확인 대상'으로만 표시합니다.</span>}>
        <div className="row" style={{ marginBottom: 8 }}>
          <label className="field"><span>학생</span>
            <select className="select" value={picked} onChange={(e) => setPicked(e.target.value === 'all' ? 'all' : Number(e.target.value))}>
              <option value="all">전체 대상자 요약</option>
              {list.map((u) => <option key={u.studentId} value={u.studentId}>{u.student}</option>)}
            </select>
          </label>
        </div>
        {all.isLoading && <span className="spin" />}
        {all.error && <Notice kind="bad">{errorMessage(all.error)}</Notice>}
        {all.data && list.length === 0 && <Empty title="대상자가 없습니다." />}
        {picked === 'all' && list.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>학생</th><th className="center">상태</th><th>한도</th><th className="num">이전 학교</th><th className="num">계산 지원액</th><th className="num">실제 사용액</th><th className="num">남은 한도</th><th className="num">경정 필요</th><th className="num">확인 대상</th><th>비고</th></tr></thead>
              <tbody>
                {list.map((u) => (
                  <tr key={u.studentId} className={u.blocked.length > 0 ? 'row--check' : u.overTotal > 0 ? 'row--error' : ''}>
                    <td>{u.student}</td>
                    <td className="center"><span className={TARGET_CLS[u.targetStatus] ?? 'badge badge--muted'}>{u.targetStatusLabel}</span></td>
                    <td>{u.limitLabel || '—'}</td>
                    <td className="num">{u.priorUsed === null ? <span className="muted">—</span> : won(u.priorUsed)}</td>
                    <td className="num">{won(u.usedExpected)}</td>
                    <td className="num">{won(u.usedActual)}</td>
                    <td className="num">{u.remaining === null ? (u.basis === 'unlimited' ? '제한 없음' : '—') : won(u.remaining)}</td>
                    <td className="num">{u.retroTotal > 0 ? won(u.retroTotal) : <span className="muted">0</span>}</td>
                    <td className="num">{u.overTotal > 0 ? won(u.overTotal) : <span className="muted">0</span>}</td>
                    <td className="wrap" style={{ fontSize: 12 }}>{[...u.blocked, ...u.warnings].join(' / ')}{u.exhaustedYm ? ` 한도 소진 ${u.exhaustedYm}` : ''}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {one && (
          <>
            <UsageSummary u={one} />
            <UsageMessages u={one} />
            <UsageRows u={one} />
          </>
        )}
      </Card>
    </>
  )
}

/* ── 소급 지원 계산 · 일괄 경정 ────────────────────────────────── */

function Retro({ yearId }: { yearId: number }) {
  const run = useRun()
  const targets = useQuery({ queryKey: ['voucherTargets', yearId], queryFn: () => api.voucherTargetList(yearId) })
  const [student, setStudent] = useState<number | 'all'>('all')
  const [decisions, setDecisions] = useState<VoucherDecision[]>([])
  const [preview, setPreview] = useState<VoucherRetroPreview | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [kind, setKind] = useState<'retro' | 'monthly'>('retro')
  const [status, setStatus] = useState<'planned' | 'done'>('done')
  const [decidedOn, setDecidedOn] = useState(today())
  const [processedOn, setProcessedOn] = useState(today())
  const [reason, setReason] = useState('')
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)

  async function calc() {
    setBusy(true)
    try {
      const p = await api.voucherRetroPreview(yearId, student === 'all' ? [] : [student], decisions)
      setPreview(p)
      setErr(null)
      setSelected(new Set(p.proposals.filter((x) => x.state === 'normal').map((x) => x.key)))
    } catch (e) {
      setErr(errorMessage(e))
      setPreview(null)
    } finally {
      setBusy(false)
    }
  }

  const decideRows = useMemo(() => (preview?.usages ?? []).flatMap((u) => u.rows.filter((r) => r.state === 'decide').map((r) => ({ u, r }))), [preview])
  const chosen = (preview?.proposals ?? []).filter((x) => x.state === 'normal' && selected.has(x.key))
  const chosenTotal = chosen.reduce((s, x) => s + x.amount, 0)

  function setDecision(studentId: number, fiscalMonthId: number, programId: number, amountItemId: number, freeAmount: number) {
    setDecisions((ds) => [...ds.filter((d) => !(d.studentId === studentId && d.fiscalMonthId === fiscalMonthId && d.programId === programId && d.amountItemId === amountItemId)), { studentId, fiscalMonthId, programId, amountItemId, freeAmount }])
  }

  async function submit() {
    if (!preview) return
    setBusy(true)
    const ok = await run(
      () =>
        api.voucherApply({
          schoolYearId: yearId,
          kind,
          reason,
          decidedOn: decidedOn || null,
          processedOn: processedOn || null,
          status,
          rows: chosen.map((x) => ({ studentId: x.studentId, fiscalMonthId: x.fiscalMonthId, programId: x.programId, amountItemId: x.amountItemId, fromFundId: x.fromFundId as number, amount: x.amount })),
          decisions,
        }),
      `경정 묶음을 등록했습니다 (${chosen.length}건, ${won(chosenTotal)}원).`,
    )
    setBusy(false)
    if (ok) {
      setConfirming(false)
      setPreview(null)
      setSelected(new Set())
      setReason('')
    }
  }

  return (
    <>
      <Card title="1. 계산 대상">
        <div className="row">
          <label className="field"><span>학생</span>
            <select className="select" value={student} onChange={(e) => setStudent(e.target.value === 'all' ? 'all' : Number(e.target.value))}>
              <option value="all">확정·종료 대상자 전체</option>
              {(targets.data ?? []).filter((t) => t.status === 'confirmed' || t.status === 'ended').map((t) => <option key={t.studentId} value={t.studentId}>{t.student}</option>)}
            </select>
          </label>
          <Button variant="primary" disabled={busy} onClick={calc} style={{ alignSelf: 'flex-end' }}>미리보기 계산</Button>
          <span className="muted" style={{ alignSelf: 'flex-end' }}>적용 시작일부터의 수강료에서 이미 자유수강권인 금액을 빼고, 남은 한도 안에서 수익자 → 자유수강권 경정을 제안합니다. 등록은 아래에서 담당자가 승인한 행만 됩니다.</span>
        </div>
        {err && <Notice kind="bad">{err}</Notice>}
      </Card>
      {preview && decideRows.length > 0 && (
        <Card title="2. 같은 달 여러 프로그램 — 프로그램별 자유수강권 배분 (담당자 결정)">
          <p className="muted" style={{ marginTop: 0 }}>남은 한도가 수강료 합보다 작아 어느 프로그램에 얼마를 쓸지 정해야 합니다. 자료에 수강 시작일이 없어 자동으로 정하지 않습니다. 배분 합이 남은 한도와 같아야 하며, 입력 뒤 '미리보기 계산'을 다시 누르세요.</p>
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>학생</th><th>귀속월</th><th>프로그램</th><th>항목</th><th className="num">지원 대상 수강료</th><th className="num">남은 한도 (달 시작)</th><th className="num">임시값</th><th className="num">배분 (원)</th></tr></thead>
              <tbody>
                {decideRows.map(({ u, r }) => {
                  const d = decisions.find((x) => x.studentId === u.studentId && x.fiscalMonthId === r.fiscalMonthId && x.programId === r.programId && x.amountItemId === r.amountItemId)
                  return (
                    <tr key={`${u.studentId}-${r.fiscalMonthId}-${r.programId}-${r.amountItemId}`}>
                      <td>{u.student}</td><td>{r.monthLabel}</td><td>{r.programName}</td><td>{r.itemName}</td>
                      <td className="num">{won(r.freeActual + r.beneficiaryActual)}</td>
                      <td className="num">{won(r.remainingBefore)}</td>
                      <td className="num muted">{won(r.freeExpected)}</td>
                      <td className="num"><input className="input" type="number" min={0} step={1} style={{ width: 120, textAlign: 'right' }} value={d?.freeAmount ?? ''} onChange={(e) => setDecision(u.studentId, r.fiscalMonthId, r.programId, r.amountItemId, Number(e.target.value))} /></td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </Card>
      )}
      {preview && (
        <Card title={`${decideRows.length > 0 ? '3' : '2'}. 미리보기 결과`} right={<span className="muted">학생 {preview.studentCount}명 · 등록 가능 {preview.normalCount}건 {won(preview.normalTotal)}원 · 그 밖 {preview.blockedCount}건</span>}>
          {preview.usages.map((u) => (
            <div key={u.studentId} style={{ marginBottom: 8 }}>
              <strong>{u.student}</strong> <span className="muted">· {u.limitLabel || '한도 없음'} · 계산 지원액 {won(u.usedExpected)} · 실제 {won(u.usedActual)} · 남은 한도 {u.remaining === null ? '—' : won(u.remaining)}</span>
              <UsageMessages u={u} />
            </div>
          ))}
          {preview.proposals.length === 0 && <Empty title="제안할 경정이 없습니다." />}
          {preview.proposals.length > 0 && (
            <div className="tableWrap">
              <table className="table">
                <thead><tr><th className="center"><input type="checkbox" checked={chosen.length > 0 && chosen.length === preview.normalCount} onChange={(e) => setSelected(e.target.checked ? new Set(preview.proposals.filter((x) => x.state === 'normal').map((x) => x.key)) : new Set())} /></th><th>학생</th><th>귀속월</th><th>프로그램</th><th>항목</th><th>변경 전 재원 → 자유수강권</th><th className="num">금액</th><th className="center">상태</th><th>사유</th></tr></thead>
                <tbody>
                  {preview.proposals.map((x: VoucherProposal) => (
                    <tr key={x.key} className={x.state === 'blocked' || x.state === 'over' ? 'row--error' : x.state !== 'normal' ? 'row--check' : ''}>
                      <td className="center">{x.state === 'normal' && <input type="checkbox" checked={selected.has(x.key)} onChange={(e) => setSelected((s) => { const n = new Set(s); if (e.target.checked) n.add(x.key); else n.delete(x.key); return n })} />}</td>
                      <td>{x.student}</td><td>{x.monthLabel}</td><td>{x.programName}</td><td>{x.itemName}</td>
                      <td>{x.fromFundName || <span className="muted">—</span>}</td>
                      <td className="num">{won(x.amount)}</td>
                      <td className="center"><span className={PROPOSAL_CLS[x.state]}>{x.stateLabel}</span></td>
                      <td className="wrap" style={{ fontSize: 12 }}>{x.reasons.join(' / ')}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {preview.normalCount > 0 && (
            <>
              <div className="upload__top" style={{ marginTop: 12 }}>
                <label className="field"><span>처리 종류</span>
                  <select className="select" value={kind} onChange={(e) => setKind(e.target.value as 'retro' | 'monthly')}><option value="retro">소급 지원 경정</option><option value="monthly">당월 사용액 경정</option></select>
                </label>
                <label className="field"><span>처리 상태</span>
                  <select className="select" value={status} onChange={(e) => setStatus(e.target.value as 'planned' | 'done')}><option value="done">처리 완료 (집계 반영)</option><option value="planned">처리 예정 (집계 제외)</option></select>
                </label>
                <label className="field"><span>결정일</span><input className="input" type="date" value={decidedOn} onChange={(e) => setDecidedOn(e.target.value)} /></label>
                <label className="field"><span>실제 처리일</span><input className="input" type="date" value={processedOn} onChange={(e) => setProcessedOn(e.target.value)} /></label>
                <label className="field" style={{ gridColumn: 'span 2' }}><span>사유 (필수)</span><input className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="예: 5월 10일 대상자 확정에 따른 3~5월 소급" /></label>
              </div>
              <div className="row" style={{ marginTop: 10, justifyContent: 'flex-end' }}>
                <span className="muted">선택 {chosen.length}건 · {won(chosenTotal)}원 → 환불 대상 {won(chosenTotal)}원 발생</span>
                <Button variant="primary" disabled={busy || chosen.length === 0 || !reason.trim()} onClick={() => setConfirming(true)}>승인 묶음 등록…</Button>
              </div>
            </>
          )}
        </Card>
      )}
      {confirming && (
        <Modal title="경정 묶음 등록 확인" onClose={() => setConfirming(false)} footer={<><Button onClick={() => setConfirming(false)}>취소</Button><Button variant="primary" disabled={busy} onClick={submit}>등록</Button></>}>
          <p style={{ marginTop: 0 }}>{kind === 'retro' ? '소급 지원 경정' : '당월 사용액 경정'} {chosen.length}건, 합계 {won(chosenTotal)}원을 {status === 'done' ? `처리 완료 (처리일 ${processedOn})` : '처리 예정'}으로 등록합니다.</p>
          <ul className="list">
            <li>등록 직전에 사용액을 다시 계산해 검증하며, 한 건이라도 막히면 전부 되돌리고 아무것도 등록하지 않습니다.</li>
            <li>각 건은 회계 정정의 재원경정(수익자 → 자유수강권)으로 남고, 그만큼 '환불 처리'에 미처리 환불 대상이 생깁니다.</li>
            <li>월별 정산·차액 분석이 다시 계산되며 기존 확인 메모는 '재확인 필요'로 표시됩니다.</li>
          </ul>
        </Modal>
      )}
    </>
  )
}

/* ── 환불 처리 ───────────────────────────────────────────────── */

function Refund({ yearId }: { yearId: number }) {
  const run = useRun()
  const list = useQuery({ queryKey: ['voucherRefunds', yearId], queryFn: () => api.voucherRefundList(yearId) })
  const [filter, setFilter] = useState('open')
  const [target, setTarget] = useState<VoucherRefundRow | null>(null)
  const [completing, setCompleting] = useState<{ row: VoucherRefundRow; id: number } | null>(null)
  const [cancelling, setCancelling] = useState<number | null>(null)
  const [amount, setAmount] = useState(0)
  const [status, setStatus] = useState<'planned' | 'done'>('done')
  const [processedOn, setProcessedOn] = useState(today())
  const [confirmed, setConfirmed] = useState(false)
  const [evidence, setEvidence] = useState('')
  const [reason, setReason] = useState('')
  const [open, setOpen] = useState<number | null>(null)

  const rows = (list.data ?? []).filter((r) => (filter === 'all' ? true : filter === 'open' ? r.status !== 'complete' && r.status !== 'cancelled' : r.status === filter))
  const openTotal = (list.data ?? []).filter((r) => r.status !== 'cancelled').reduce((s, r) => s + r.outstanding, 0)

  function startRegister(r: VoucherRefundRow) {
    setTarget(r)
    setAmount(r.outstanding)
    setStatus('done')
    setProcessedOn(today())
    setConfirmed(false)
    setEvidence('')
    setReason('')
  }
  return (
    <>
      <Card title="환불 대상 (수익자 → 자유수강권 경정에 따른 반환)" right={<span className="muted">미반환 합계 {won(openTotal)}원</span>}>
        <div className="row" style={{ marginBottom: 8 }}>
          <label className="field"><span>상태</span>
            <select className="select" value={filter} onChange={(e) => setFilter(e.target.value)}>
              <option value="open">미처리·처리 예정·일부 완료</option><option value="all">전체</option><option value="none">미처리</option><option value="planned">처리 예정</option><option value="partial">일부 완료</option><option value="complete">처리 완료</option><option value="cancelled">취소</option>
            </select>
          </label>
          <span className="muted" style={{ alignSelf: 'flex-end' }}>환불은 자동으로 완료되지 않습니다. 완료 처리에는 실제 수납(학부모가 낸 돈) 근거 확인이 필요하며, 원장 징수액은 다시 차감되지 않습니다.</span>
        </div>
        {list.error && <Notice kind="bad">{errorMessage(list.error)}</Notice>}
        {list.data && rows.length === 0 && <Empty title="조건에 맞는 환불 대상이 없습니다." />}
        {rows.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>경정</th><th>학생</th><th>귀속월</th><th>프로그램</th><th>항목</th><th>변경 전 재원</th><th className="num">경정 금액</th><th className="num">반환 완료</th><th className="num">반환 예정</th><th className="num">미반환</th><th className="center">상태</th><th /></tr></thead>
              <tbody>
                {rows.map((r) => (
                  <>
                    <tr key={r.transferId} className={r.orphan ? 'row--error' : r.status === 'cancelled' ? 'row--off' : ''}>
                      <td>#{r.transferId}{r.fromBatch ? <span className="muted"> 묶음 {r.batchId}</span> : <span className="muted"> 직접</span>}{r.transferStatus !== 'done' ? <span className="badge badge--soon" style={{ marginLeft: 4 }}>{r.transferStatusLabel}</span> : null}{r.orphan && <span className="badge badge--bad" style={{ marginLeft: 4 }}>원본 재확인 필요</span>}</td>
                      <td>{r.student}</td><td>{r.monthLabel}</td><td>{r.programName}</td><td>{r.itemName}</td><td>{r.fromFundName}</td>
                      <td className="num">{won(r.amount)}</td><td className="num">{won(r.returnedDone)}</td><td className="num">{won(r.returnedPlanned)}</td><td className="num"><strong>{won(r.outstanding)}</strong></td>
                      <td className="center"><span className={REFUND_CLS[r.status]}>{r.statusLabel}</span></td>
                      <td>
                        <div className="row" style={{ gap: 4, flexWrap: 'nowrap' }}>
                          {r.status !== 'complete' && r.status !== 'cancelled' && r.transferStatus === 'done' && <Button size="sm" variant="primary" onClick={() => startRegister(r)}>환불 등록…</Button>}
                          {r.returns.length > 0 && <Button size="sm" onClick={() => setOpen(open === r.transferId ? null : r.transferId)}>내역 {r.returns.length}</Button>}
                        </div>
                      </td>
                    </tr>
                    {open === r.transferId && (
                      <tr key={`${r.transferId}-ret`}>
                        <td colSpan={12} style={{ background: 'var(--bg-sub, #f7f7f9)' }}>
                          <table className="table" style={{ width: 'auto' }}>
                            <thead><tr><th>반환</th><th className="num">금액</th><th className="center">상태</th><th>처리일</th><th>수납 확인</th><th>근거</th><th /></tr></thead>
                            <tbody>
                              {r.returns.map((x) => (
                                <tr key={x.adjustmentId} className={x.status === 'cancelled' ? 'row--off' : ''}>
                                  <td>#{x.adjustmentId}</td><td className="num">{won(x.amount)}</td><td className="center"><span className={x.status === 'done' ? 'badge badge--ok' : x.status === 'planned' ? 'badge badge--soon' : 'badge badge--muted'}>{x.statusLabel}</span></td>
                                  <td>{x.processedOn ?? '—'}</td><td>{x.collectedConfirmed ? '확인됨' : <span className="muted">미확인</span>}</td><td className="wrap">{x.evidence}</td>
                                  <td>
                                    <div className="row" style={{ gap: 4, flexWrap: 'nowrap' }}>
                                      {x.status === 'planned' && <Button size="sm" onClick={() => { setCompleting({ row: r, id: x.adjustmentId }); setProcessedOn(today()); setConfirmed(false); setEvidence('') }}>완료 처리…</Button>}
                                      {x.status !== 'cancelled' && <Button size="sm" onClick={() => { setCancelling(x.adjustmentId); setReason('') }}>취소…</Button>}
                                    </div>
                                  </td>
                                </tr>
                              ))}
                            </tbody>
                          </table>
                        </td>
                      </tr>
                    )}
                  </>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {target && (
        <Modal title={`환불 등록 — ${target.student} · ${target.monthLabel} · ${target.programName}`} onClose={() => setTarget(null)} footer={<><Button onClick={() => setTarget(null)}>닫기</Button><Button variant="primary" disabled={amount <= 0 || !reason.trim() || (status === 'done' && (!confirmed || !evidence.trim()))} onClick={() => run(() => api.voucherRefundRegister({ transferId: target.transferId, amount: Math.trunc(amount), status, processedOn: processedOn || null, collectedConfirmed: confirmed, evidence, reason }), '환불을 등록했습니다.', () => setTarget(null))}>등록</Button></>}>
          <div className="upload__top">
            <label className="field"><span>환불 금액 (원) — 미반환 {won(target.outstanding)}</span><input className="input" type="number" min={1} max={target.outstanding} step={1} value={amount || ''} onChange={(e) => setAmount(Number(e.target.value))} /></label>
            <label className="field"><span>처리 상태</span>
              <select className="select" value={status} onChange={(e) => setStatus(e.target.value as 'planned' | 'done')}><option value="done">처리 완료 (실제 반환됨)</option><option value="planned">처리 예정</option></select>
            </label>
            <label className="field"><span>실제 처리일</span><input className="input" type="date" value={processedOn} onChange={(e) => setProcessedOn(e.target.value)} /></label>
            <label className="field"><span>실제 수납 확인</span>
              <label className="row" style={{ height: 'var(--h-control, 28px)' }}><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> 학부모가 실제로 낸 금액임을 수납 자료로 확인했습니다</label>
            </label>
            <label className="field" style={{ gridColumn: 'span 2' }}><span>수납 근거 (완료 때 필수)</span><input className="input" value={evidence} onChange={(e) => setEvidence(e.target.value)} placeholder="예: 스쿨뱅킹 수납 내역 2026-04-10" /></label>
            <label className="field" style={{ gridColumn: 'span 2' }}><span>사유 (필수)</span><input className="input" value={reason} onChange={(e) => setReason(e.target.value)} placeholder="예: 소급 경정에 따른 수강료 반환" /></label>
          </div>
          <p className="muted" style={{ marginBottom: 0 }}>반환 이행은 회계 정정의 '경정 반환 이행'으로 등록되며 원장 징수액을 다시 차감하지 않습니다 (재원경정이 이미 수익자 금액을 줄였습니다).</p>
        </Modal>
      )}
      {completing && (
        <Modal title={`환불 #${completing.id} 완료 처리`} onClose={() => setCompleting(null)} footer={<><Button onClick={() => setCompleting(null)}>닫기</Button><Button variant="primary" disabled={!processedOn || !confirmed || !evidence.trim()} onClick={() => run(() => api.voucherRefundComplete(completing.id, processedOn, confirmed, evidence), '환불을 완료 처리했습니다.', () => setCompleting(null))}>완료</Button></>}>
          <div className="row"><span className="label">실제 처리일</span><input className="input" type="date" value={processedOn} onChange={(e) => setProcessedOn(e.target.value)} /></div>
          <div className="row" style={{ marginTop: 8 }}><label className="row"><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> 실제 수납 근거를 확인했습니다</label></div>
          <div className="row" style={{ marginTop: 8 }}><span className="label">수납 근거</span><input className="input" style={{ flex: 1 }} value={evidence} onChange={(e) => setEvidence(e.target.value)} /></div>
        </Modal>
      )}
      {cancelling !== null && (
        <Modal title={`환불 #${cancelling} 취소`} onClose={() => setCancelling(null)} footer={<><Button onClick={() => setCancelling(null)}>닫기</Button><Button variant="primary" disabled={!reason.trim()} onClick={() => run(() => api.adjustmentCancel(cancelling, reason), '취소했습니다.', () => setCancelling(null))}>취소 실행</Button></>}>
          <div className="row"><span className="label">취소 사유</span>
            <select className="select" value={reason} onChange={(e) => setReason(e.target.value)} style={{ flex: 1 }}>
              <option value="">— 선택 —</option>
              {['학생 선택 오류', '금액 입력 오류', '중복 등록', '실제 회계 처리 취소'].map((r) => <option key={r} value={r}>{r}</option>)}
            </select>
          </div>
        </Modal>
      )}
    </>
  )
}

/* ── 전입생 관리 ─────────────────────────────────────────────── */

function TransferIn({ yearId }: { yearId: number }) {
  const run = useRun()
  const targets = useQuery({ queryKey: ['voucherTargets', yearId], queryFn: () => api.voucherTargetList(yearId) })
  const [editing, setEditing] = useState<VoucherTarget | null>(null)
  const [prior, setPrior] = useState<number | ''>('')
  const [confirmed, setConfirmed] = useState(false)
  const [note, setNote] = useState('')
  const rows = (targets.data ?? []).filter((t) => t.transferIn)
  return (
    <>
      <Card title="전입생 — 이전 학교 사용액" right={<span className="muted">확인 전에는 사용액을 계산하지 않습니다. 0원도 담당자가 명시해야 확인됩니다.</span>}>
        {targets.data && rows.length === 0 && <Empty title="전입생으로 등록된 대상자가 없습니다.">대상자 관리에서 '전입생'을 켜고 전입일을 입력하면 여기에 나타납니다.</Empty>}
        {rows.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>학생</th><th className="center">대상자 상태</th><th>전입일</th><th>적용 시작</th><th className="num">이전 학교 사용액</th><th className="center">확인 상태</th><th>메모</th><th /></tr></thead>
              <tbody>
                {rows.map((t) => (
                  <tr key={t.id} className={t.priorNeedsConfirm ? 'row--check' : ''}>
                    <td>{t.student}</td>
                    <td className="center"><span className={TARGET_CLS[t.status]}>{t.statusLabel}</span></td>
                    <td>{t.transferInOn}</td><td>{t.applyFrom ?? '—'}</td>
                    <td className="num">{t.priorUsed === null ? <span className="muted">미입력</span> : won(t.priorUsed)}</td>
                    <td className="center"><span className={t.priorNeedsConfirm ? 'badge badge--bad' : 'badge badge--ok'}>{t.priorStatusLabel}</span></td>
                    <td className="wrap" style={{ fontSize: 12 }}>{t.priorNote}</td>
                    <td><Button size="sm" variant="primary" onClick={() => { setEditing(t); setPrior(t.priorUsed ?? ''); setConfirmed(t.priorStatus === 'confirmed'); setNote(t.priorNote) }}>사용액 입력…</Button></td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      {editing && (
        <Modal title={`이전 학교 사용액 — ${editing.student}`} onClose={() => setEditing(null)} footer={<><Button onClick={() => setEditing(null)}>닫기</Button><Button variant="primary" onClick={() => run(() => api.voucherPriorSet({ targetId: editing.id, priorUsed: prior === '' ? null : Math.trunc(prior), confirmed, note }), '이전 학교 사용액을 저장했습니다.', () => setEditing(null))}>저장</Button></>}>
          <div className="row"><span className="label">이전 학교 사용액 (원)</span><input className="input" type="number" min={0} step={1} value={prior} onChange={(e) => setPrior(e.target.value === '' ? '' : Number(e.target.value))} /></div>
          <div className="row" style={{ marginTop: 8 }}><label className="row"><input type="checkbox" checked={confirmed} onChange={(e) => setConfirmed(e.target.checked)} /> 이전 학교 자료로 확인했습니다 (확인 전에는 계산하지 않음)</label></div>
          <div className="row" style={{ marginTop: 8 }}><span className="label">근거 메모</span><input className="input" style={{ flex: 1 }} value={note} onChange={(e) => setNote(e.target.value)} placeholder="예: 전출교 공문 2026-09-03" /></div>
          <p className="muted">연간 한도면 연간 한도에서, 학기별 한도면 전입한 학기의 한도에서 뺍니다.</p>
        </Modal>
      )}
    </>
  )
}

/* ── 처리 이력 ───────────────────────────────────────────────── */

function History({ yearId }: { yearId: number }) {
  const h = useQuery({ queryKey: ['voucherHistory', yearId], queryFn: () => api.voucherHistory(yearId) })
  const [open, setOpen] = useState<number | null>(null)
  if (h.error) return <Notice kind="bad">{errorMessage(h.error)}</Notice>
  if (!h.data) return <span className="spin" />
  const d = h.data
  return (
    <>
      <Card title="경정 묶음 (소급 · 당월)">
        {d.batches.length === 0 && <Empty title="등록된 묶음이 없습니다." />}
        {d.batches.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>묶음</th><th>등록</th><th>종류</th><th>학생</th><th className="num">건수</th><th className="num">합계</th><th>사유</th><th /></tr></thead>
              <tbody>
                {d.batches.map((b) => (
                  <>
                    <tr key={b.id}>
                      <td>#{b.id}</td><td className="muted">{shortDateTime(b.createdAt)}</td><td>{b.kindLabel}</td><td>{b.student || <span className="muted">여러 학생</span>}</td><td className="num">{b.itemCount}</td><td className="num">{won(b.totalAmount)}</td><td className="wrap">{b.reason}</td>
                      <td><Button size="sm" onClick={() => setOpen(open === b.id ? null : b.id)}>{open === b.id ? '닫기' : '내역'}</Button></td>
                    </tr>
                    {open === b.id && (
                      <tr key={`${b.id}-items`}>
                        <td colSpan={8} style={{ background: 'var(--bg-sub, #f7f7f9)' }}>
                          <table className="table" style={{ width: 'auto' }}>
                            <thead><tr><th>경정</th><th>학생</th><th>귀속월</th><th>프로그램</th><th>항목</th><th>변경 전 재원</th><th className="num">금액</th><th className="center">상태</th></tr></thead>
                            <tbody>{b.items.map((i) => <tr key={i.adjustmentId}><td>#{i.adjustmentId}</td><td>{i.student}</td><td>{i.monthLabel}</td><td>{i.programName}</td><td>{i.itemName}</td><td>{i.fromFundName} → 자유수강권</td><td className="num">{won(i.amount)}</td><td className="center"><span className={i.status === 'done' ? 'badge badge--ok' : i.status === 'planned' ? 'badge badge--soon' : 'badge badge--muted'}>{i.statusLabel}</span></td></tr>)}</tbody>
                          </table>
                          <p className="muted" style={{ margin: '4px 0 0' }}>각 경정의 완료·취소는 '회계 정정 → 정정 이력'에서 처리합니다.</p>
                        </td>
                      </tr>
                    )}
                  </>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
      <Card title="대상자 변경 이력">
        {d.targetLogs.length === 0 && <Empty title="이력이 없습니다." />}
        {d.targetLogs.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>일시</th><th>학생</th><th>작업</th><th>상태</th><th>내용</th></tr></thead>
              <tbody>{d.targetLogs.map((l, i) => <tr key={i}><td className="muted">{shortDateTime(l.at)}</td><td>{l.student}</td><td>{LOG_ACTION[l.action] ?? l.action}</td><td>{l.fromStatus || l.toStatus ? `${labelOf(l.fromStatus)} → ${labelOf(l.toStatus)}` : '—'}</td><td className="wrap">{l.detail}</td></tr>)}</tbody>
            </table>
          </div>
        )}
      </Card>
      <Card title="정책 · 예외 이력">
        <div className="tableWrap">
          <table className="table">
            <thead><tr><th>등록</th><th>구분</th><th>대상</th><th>한도</th><th>메모·사유</th><th className="center">상태</th></tr></thead>
            <tbody>
              {d.policyHistory.map((p) => <tr key={`p${p.id}`} className={p.supersededAt ? 'row--off' : ''}><td className="muted">{shortDateTime(p.createdAt)}</td><td>학년도 정책</td><td>전체</td><td>{p.basis === 'annual' ? `연간 ${won(p.annualLimit)}` : p.basis === 'semester' ? `1학기 ${won(p.semester1Limit)} · 2학기 ${won(p.semester2Limit)}${p.carryOver ? ' · 이월' : ''}` : '제한 없음'}</td><td className="wrap">{p.note}</td><td className="center">{p.supersededAt ? <span className="badge badge--muted">대체됨</span> : <span className="badge badge--ok">현재</span>}</td></tr>)}
              {d.exceptionHistory.map((x) => <tr key={`x${x.id}`} className={x.supersededAt ? 'row--off' : ''}><td className="muted">{shortDateTime(x.createdAt)}</td><td>학생 예외</td><td>{x.student}</td><td>{x.basis === 'annual' ? `연간 ${won(x.annualLimit)}` : x.basis === 'semester' ? `1학기 ${won(x.semester1Limit)} · 2학기 ${won(x.semester2Limit)}${x.carryOver ? ' · 이월' : ''}` : '제한 없음'}</td><td className="wrap">{x.reason}</td><td className="center">{x.supersededAt ? <span className="badge badge--muted">해제됨</span> : <span className="badge badge--ok">현재</span>}</td></tr>)}
            </tbody>
          </table>
        </div>
      </Card>
    </>
  )
}
