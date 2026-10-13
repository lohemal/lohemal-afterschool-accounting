import { useQuery, useQueryClient } from '@tanstack/react-query'
import { open, save } from '@tauri-apps/plugin-dialog'
import { useEffect, useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { bytes, shortDateTime, won } from '@/lib/format'
import { currentYm } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { BackupInfo, DbCounts, IntegrityReport, RestorePreview, RestoreResult } from '@/ipc/types'

type Tab = 'backup' | 'restore' | 'integrity' | 'export' | 'history'

const TABS: [Tab, string][] = [
  ['backup', '백업'],
  ['restore', '복원'],
  ['integrity', '무결성 검사'],
  ['export', '자료 내보내기'],
  ['history', '작업 이력'],
]

export function DataPage() {
  const { year } = useApp()
  const [tab, setTab] = useState<Tab>('backup')
  return (
    <Page title="데이터 관리" sub="백업은 만든 뒤 검증하고, 복원은 미리보기와 최종 확인을 거칩니다. 무결성 검사는 자료를 고치지 않고 알려만 줍니다. 백업·내보내기 파일에는 학생 개인정보가 들어 있으니 보관 위치와 외부 반출에 주의하세요.">
      <div className="tabs">
        {TABS.map(([k, l]) => (
          <button key={k} type="button" className={tab === k ? 'tab tab--on' : 'tab'} onClick={() => setTab(k)}>
            {l}
          </button>
        ))}
      </div>
      {tab === 'backup' && <Backup />}
      {tab === 'restore' && <Restore />}
      {tab === 'integrity' && (year ? <Integrity yearId={year.id} /> : <Notice>학년도가 없습니다.</Notice>)}
      {tab === 'export' && (year ? <Export yearId={year.id} /> : <Notice>학년도가 없습니다.</Notice>)}
      {tab === 'history' && <History />}
    </Page>
  )
}

function CountsTable({ a, b, al, bl }: { a: DbCounts; b: DbCounts | null; al: string; bl: string }) {
  const rows: [string, (c: DbCounts) => string][] = [
    ['DB 스키마', (c) => `v${c.schemaVersion}`],
    ['학년도', (c) => c.years.join(', ') || '—'],
    ['원장 거래 (전체 / 유효)', (c) => `${won(c.ledgerEntries)} / ${won(c.activeEntries)}건`],
    ['회계 정정 (재원경정·환불·추가징수)', (c) => `${won(c.adjustments)}건`],
    ['환불 이력', (c) => `${won(c.refunds)}건`],
    ['자유수강권 대상자', (c) => `${won(c.voucherTargets)}명`],
    ['자유수강권 승인 묶음', (c) => `${won(c.voucherBatches)}건`],
    ['업로드 파일', (c) => `${won(c.uploads)}건`],
    ['학생', (c) => `${won(c.students)}명`],
    ['월 확정·재개 이력', (c) => `${won(c.monthCloses)}건`],
    ['감사 로그', (c) => `${won(c.auditLogs)}건`],
  ]
  return (
    <div className="tableWrap">
      <table className="table">
        <thead><tr><th>항목</th><th>{al}</th><th>{bl}</th></tr></thead>
        <tbody>
          {rows.map(([l, f]) => (
            <tr key={l}><td>{l}</td><td>{f(a)}</td><td>{b ? f(b) : <span className="muted">—</span>}</td></tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

/* ── 백업 ────────────────────────────────────────────────────── */

function Backup() {
  const toast = useToast()
  const qc = useQueryClient()
  const list = useQuery({ queryKey: ['backups'], queryFn: api.backupList })
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [auto, setAuto] = useState(true)
  const [keep, setKeep] = useState(10)
  useEffect(() => {
    if (list.data) {
      setAuto(list.data.autoEnabled)
      setKeep(list.data.keep)
    }
  }, [list.data])
  async function create() {
    setBusy(true)
    try {
      const b = await api.backupCreate(note)
      toast.show(`백업을 만들고 검증했습니다: ${b.fileName}`)
      setNote('')
      await qc.invalidateQueries({ queryKey: ['backups'] })
      await qc.invalidateQueries({ queryKey: ['opsHistory'] })
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }
  async function saveSettings() {
    try {
      await api.backupSettingsSet(auto, keep)
      toast.show('자동 백업 설정을 저장했습니다.')
      await qc.invalidateQueries({ queryKey: ['backups'] })
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }
  const d = list.data
  const Row = ({ b }: { b: BackupInfo }) => (
    <tr className={b.verified ? '' : 'row--error'}>
      <td>{b.createdAt || <span className="muted">—</span>}</td>
      <td>{b.fileName}</td>
      <td>{b.kindLabel}{b.protected && !b.legacy && <span className="badge badge--muted" style={{ marginLeft: 4 }}>보존</span>}</td>
      <td className="num">{bytes(b.size)}</td>
      <td>v{b.schemaVersion || '?'} · {b.appVersion || '?'}</td>
      <td className="center">{b.verified ? <span className="badge badge--ok">검증됨</span> : <span className="badge badge--bad">{b.legacy ? '이전 형식' : '검증 실패'}</span>}{!b.complete && b.verified && <span className="badge badge--soon" style={{ marginLeft: 4 }}>원본 일부 없음</span>}</td>
      <td className="wrap" style={{ fontSize: 12 }}>{b.counts ? `원장 ${won(b.counts.ledgerEntries)}건 · 정정 ${won(b.counts.adjustments)}건 · 대상자 ${won(b.counts.voucherTargets)}명` : b.verifyErrors.join(' / ')}</td>
    </tr>
  )
  return (
    <>
      <Card title="수동 백업" right={d ? <span className="muted">최근 백업: {d.lastBackupAt ?? '없음'} · 폴더 {d.dir}</span> : undefined}>
        <div className="row">
          <label className="field" style={{ flex: 1 }}><span>메모 (선택)</span><input className="input" value={note} onChange={(e) => setNote(e.target.value)} placeholder="예: 9월 정산 전" /></label>
          <Button variant="primary" disabled={busy} onClick={create} style={{ alignSelf: 'flex-end' }}>{busy ? '백업 중…' : '지금 백업'}</Button>
        </div>
        <p className="muted" style={{ marginBottom: 0 }}>실행 중인 DB 를 복사하지 않고 SQLite 백업 API 로 일관된 스냅샷을 뜬 뒤, 보관 중인 원본 엑셀과 함께 하나의 zip 으로 묶고 체크섬·무결성·메타데이터를 검증합니다. 검증을 통과한 백업만 아래 목록에 보입니다.</p>
      </Card>
      <Card title="자동 백업 설정">
        <div className="row">
          <label className="row"><input type="checkbox" checked={auto} onChange={(e) => setAuto(e.target.checked)} /> 프로그램을 시작할 때 하루 한 번 자동 백업</label>
          <label className="field"><span>자동 백업 보관 개수</span><input className="input" type="number" min={1} max={100} value={keep} onChange={(e) => setKeep(Number(e.target.value))} style={{ width: 90 }} /></label>
          <Button onClick={saveSettings} style={{ alignSelf: 'flex-end' }}>설정 저장</Button>
        </div>
        <p className="muted" style={{ marginBottom: 0 }}>자동 백업은 최근 N개만 남기고 정리합니다. 수동 백업·마이그레이션 직전 백업·복원 직전 안전 백업은 자동으로 지우지 않습니다. 정리 내역은 작업 이력에 남습니다.</p>
      </Card>
      <Card title="백업 목록">
        {list.error && <Notice kind="bad">{errorMessage(list.error)}</Notice>}
        {d && d.backups.length === 0 && <Empty title="검증된 백업이 없습니다." />}
        {d && d.backups.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>생성</th><th>파일</th><th>유형</th><th className="num">크기</th><th>스키마 · 앱</th><th className="center">검증</th><th>내용</th></tr></thead>
              <tbody>{d.backups.map((b) => <Row key={b.path} b={b} />)}</tbody>
            </table>
          </div>
        )}
        {d && d.failed.length > 0 && (
          <>
            <p className="muted" style={{ marginTop: 10 }}>검증 실패 · 미검증 · 이전 형식 ({d.failed.length}건) — 정상 목록에 넣지 않으며 복원에 쓸 수 없습니다.</p>
            <div className="tableWrap">
              <table className="table">
                <thead><tr><th>생성</th><th>파일</th><th>유형</th><th className="num">크기</th><th>스키마 · 앱</th><th className="center">검증</th><th>사유</th></tr></thead>
                <tbody>{d.failed.map((b) => <Row key={b.path} b={b} />)}</tbody>
              </table>
            </div>
          </>
        )}
      </Card>
    </>
  )
}

/* ── 복원 ────────────────────────────────────────────────────── */

function Restore() {
  const toast = useToast()
  const qc = useQueryClient()
  const [path, setPath] = useState('')
  const [pv, setPv] = useState<RestorePreview | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState(false)
  const [typed, setTyped] = useState('')
  const [result, setResult] = useState<RestoreResult | null>(null)
  async function pick() {
    const p = await open({ multiple: false, filters: [{ name: '백업 패키지', extensions: ['zip'] }] })
    if (typeof p === 'string') {
      setPath(p)
      setPv(null)
      setResult(null)
      setBusy(true)
      try {
        setPv(await api.restoreInspect(p))
      } catch (e) {
        toast.show(errorMessage(e), true)
      } finally {
        setBusy(false)
      }
    }
  }
  async function run() {
    setBusy(true)
    try {
      const r = await api.restoreApply(path, true)
      setResult(r)
      setConfirming(false)
      toast.show(r.ok ? '복원이 끝났습니다.' : r.message, !r.ok)
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }
  return (
    <>
      <Card title="1. 복원 파일 선택">
        <div className="row">
          <Button variant="primary" onClick={pick} disabled={busy}>백업 파일 선택…</Button>
          <span className="muted">{path || '검증된 백업 패키지(.zip)를 고르면 검증·비교 결과가 아래에 나옵니다. 고르기만 해서는 아무것도 바뀌지 않습니다.'}</span>
        </div>
      </Card>
      {pv && (
        <Card title="2. 백업 정보 · 현재 자료와 비교 · 복원 영향 미리보기">
          {pv.manifest && (
            <p style={{ marginTop: 0 }}>
              생성 {pv.manifest.createdAt} · {pv.manifest.kind} · 앱 {pv.manifest.appVersion} · 스키마 v{pv.manifest.schemaVersion} · 파일 {pv.manifest.files.length}개{pv.manifest.note ? ` · ${pv.manifest.note}` : ''}
            </p>
          )}
          <div className="row" style={{ marginBottom: 8 }}>
            <span className={pv.verify.ok ? 'badge badge--ok' : 'badge badge--bad'}>{pv.verify.ok ? '패키지 검증 통과' : '패키지 검증 실패'}</span>
            {pv.verify.errors.map((e) => <span key={e} className="muted">{e}</span>)}
          </div>
          <CountsTable a={pv.current} b={pv.backup} al="현재 데이터" bl="복원할 데이터" />
          <Notice kind="bad"><div><strong>복원하면 현재 자료는 백업 시점으로 되돌아갑니다.</strong> 백업 이후에 등록한 거래·정정·대상자·메모는 사라집니다. 복원 직전에 현재 자료의 안전 백업을 자동으로 만들고, 복원 중 실패하거나 복원 뒤 검증에 실패하면 기존 자료를 그대로 둡니다.</div></Notice>
          {pv.warnings.map((w) => <div key={w} className="notice notice--soon" style={{ marginTop: 6 }}>{w}</div>)}
          {pv.blocked.map((b) => <Notice key={b} kind="bad">{b}</Notice>)}
          <div className="row" style={{ marginTop: 10, justifyContent: 'flex-end' }}>
            <Button variant="primary" disabled={!pv.canRestore || busy} onClick={() => { setTyped(''); setConfirming(true) }}>전체 복원…</Button>
          </div>
        </Card>
      )}
      {result && (
        <Card title="3. 복원 결과">
          <Notice kind={result.ok ? 'ok' : 'bad'}><div>{result.message}{result.safetyBackup ? ` · 복원 직전 안전 백업: ${result.safetyBackup}` : ''}</div></Notice>
          <CountsTable a={result.before} b={result.after} al="복원 전" bl="복원 후" />
          <p className="muted">복원 전 DB 식별 {result.beforeDbSha256.slice(0, 12)}… · 복원 후 DB 식별 {result.afterDbSha256.slice(0, 12) || '—'}… (작업 이력에 전체 값이 남습니다)</p>
        </Card>
      )}
      {confirming && pv && (
        <Modal title="최종 확인 — 전체 복원" onClose={() => setConfirming(false)} footer={<><Button onClick={() => setConfirming(false)}>취소</Button><Button variant="primary" disabled={typed !== '복원' || busy} onClick={run}>{busy ? '복원 중…' : '복원 실행'}</Button></>}>
          <p style={{ marginTop: 0 }}>현재 자료를 <strong>{pv.manifest?.createdAt ?? ''}</strong> 백업 시점으로 되돌립니다. 되돌리기 어려운 작업입니다.</p>
          <ul className="list">
            <li>복원 직전에 현재 자료의 안전 백업을 만들고 검증합니다 (실패하면 복원하지 않습니다).</li>
            <li>복원 중에는 다른 회계 쓰기가 막힙니다.</li>
            <li>복원 뒤 무결성 검사에 실패하면 기존 자료로 되돌립니다.</li>
          </ul>
          <div className="row"><span className="label">확인을 위해 '복원' 을 입력</span><input className="input" value={typed} onChange={(e) => setTyped(e.target.value)} /></div>
        </Modal>
      )}
    </>
  )
}

/* ── 무결성 검사 ───────────────────────────────────────────── */

const LEVEL_CLS: Record<string, string> = { ok: 'badge badge--ok', check: 'badge badge--soon', error: 'badge badge--bad' }
const LEVEL_LABEL: Record<string, string> = { ok: '정상', check: '확인 필요', error: '오류' }

function Integrity({ yearId }: { yearId: number }) {
  const toast = useToast()
  const qc = useQueryClient()
  const [rep, setRep] = useState<IntegrityReport | null>(null)
  const [busy, setBusy] = useState(false)
  const [filter, setFilter] = useState('all')
  async function run() {
    setBusy(true)
    try {
      setRep(await api.integrityRun(yearId))
      await qc.invalidateQueries({ queryKey: ['opsHistory'] })
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }
  async function exportXlsx() {
    try {
      const name = await api.exportDefaultName('integrity', yearId)
      const p = await save({ defaultPath: name, filters: [{ name: 'Excel', extensions: ['xlsx'] }] })
      if (!p) return
      const r = await api.exportIntegrity(yearId, p)
      toast.show(`무결성 검사 결과 ${r.rows}행을 내보냈습니다.`)
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }
  const items = (rep?.items ?? []).filter((i) => filter === 'all' || i.level === filter)
  return (
    <>
      <Card title="전체 검사" right={rep ? <span className="muted">검사일 {rep.checkedAt}</span> : undefined}>
        <div className="row">
          <Button variant="primary" disabled={busy} onClick={run}>{busy ? '검사 중…' : '전체 검사 실행'}</Button>
          {rep && <Button onClick={exportXlsx}>Excel 내보내기…</Button>}
          <span className="muted">원장·재원경정·환불·반환 이행·자유수강권 한도·기간·월 확정·원본 연결·배분·감사 로그를 검사합니다. 아무것도 자동으로 고치지 않습니다.</span>
        </div>
        {rep && (
          <div className="row" style={{ marginTop: 10 }}>
            <div className="stat"><span>정상 (검사 항목)</span><strong>{rep.okCount}</strong></div>
            <div className="stat stat--warn"><span>확인 필요</span><strong>{rep.checkCount}</strong></div>
            <div className="stat stat--bad"><span>오류</span><strong>{rep.errorCount}</strong></div>
          </div>
        )}
      </Card>
      {rep && (
        <Card title="검사 항목 요약">
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>검사 항목</th><th className="center">등급</th><th className="num">건수</th></tr></thead>
              <tbody>{rep.checks.map((c) => <tr key={c.code}><td>{c.title}</td><td className="center"><span className={LEVEL_CLS[c.level]}>{LEVEL_LABEL[c.level]}</span></td><td className="num">{c.count}</td></tr>)}</tbody>
            </table>
          </div>
        </Card>
      )}
      {rep && (
        <Card title="상세 결과" right={<select className="select" value={filter} onChange={(e) => setFilter(e.target.value)}><option value="all">전체</option><option value="error">오류</option><option value="check">확인 필요</option></select>}>
          {items.length === 0 && <Empty title="해당 등급의 항목이 없습니다." />}
          {items.length > 0 && (
            <div className="tableWrap">
              <table className="table">
                <thead><tr><th className="center">등급</th><th>항목</th><th>내용</th><th>학생</th><th>프로그램</th><th>귀속월</th><th>관련 거래</th><th>예상 원인</th><th>확인 방법</th><th>수정 안내</th></tr></thead>
                <tbody>
                  {items.map((i, k) => (
                    <tr key={k} className={i.level === 'error' ? 'row--error' : 'row--check'}>
                      <td className="center"><span className={LEVEL_CLS[i.level]}>{LEVEL_LABEL[i.level]}</span></td>
                      <td>{i.title}</td><td className="wrap">{i.detail}</td><td>{i.student}</td><td>{i.program}</td><td>{i.month}</td>
                      <td>{i.entryIds.map((e) => `#${e}`).join(', ')}</td>
                      <td className="wrap" style={{ fontSize: 12 }}>{i.cause}</td><td className="wrap" style={{ fontSize: 12 }}>{i.howToCheck}</td><td className="wrap" style={{ fontSize: 12 }}>{i.fixHint}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      )}
    </>
  )
}

/* ── 자료 내보내기 ────────────────────────────────────────── */

function Export({ yearId }: { yearId: number }) {
  const toast = useToast()
  const qc = useQueryClient()
  const months = useQuery({ queryKey: ['months', yearId], queryFn: () => api.monthList(yearId) })
  const [monthId, setMonthId] = useState<number | ''>('')
  const [status, setStatus] = useState('all')
  const [item, setItem] = useState('all')
  const [program, setProgram] = useState('')
  const [refundStatus, setRefundStatus] = useState('all')
  const [refundFund, setRefundFund] = useState('all')
  useEffect(() => {
    if (monthId === '' && months.data?.length) {
      const today = currentYm(new Date())
      setMonthId((months.data.find((m) => m.ym === today) ?? months.data[0]).id)
    }
  }, [months.data, monthId])
  async function go(kind: 'recon' | 'usage' | 'refunds' | 'integrity') {
    try {
      const name = await api.exportDefaultName(kind, yearId, kind === 'recon' && monthId !== '' ? monthId : null)
      const p = await save({ defaultPath: name, filters: [{ name: 'Excel', extensions: ['xlsx'] }] })
      if (!p) return
      const r =
        kind === 'recon'
          ? await api.exportRecon(monthId as number, { status, itemSet: item, programText: program }, p)
          : kind === 'usage'
            ? await api.exportVoucherUsage(yearId, p)
            : kind === 'refunds'
              ? await api.exportVoucherRefunds(yearId, { status: refundStatus, fundName: refundFund }, p)
              : await api.exportIntegrity(yearId, p)
      toast.show(`'${r.sheet}' ${r.rows}행을 내보냈습니다: ${r.path}`)
      await qc.invalidateQueries({ queryKey: ['opsHistory'] })
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }
  return (
    <>
      <Notice>내보낸 Excel 파일에는 학생 이름 등 개인정보가 들어 있습니다. 금액은 숫자, 날짜는 YYYY-MM-DD, 합계 행이 포함되며 파일명에 학년도와 출력일이 들어갑니다.</Notice>
      <Card title="A. 월별 회계 검증 결과">
        <div className="row">
          <label className="field"><span>월</span>
            <select className="select" value={monthId} onChange={(e) => setMonthId(Number(e.target.value))}>{(months.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.label} ({m.ym})</option>)}</select>
          </label>
          <label className="field"><span>검증 상태</span>
            <select className="select" value={status} onChange={(e) => setStatus(e.target.value)}>
              <option value="all">전체</option><option value="attention">확인이 필요한 것만</option><option value="mismatch">불일치</option><option value="ok">정상</option><option value="missing">자료 부족</option><option value="unallocated">배분 미확정</option><option value="unmapped">매핑 미확정</option><option value="orphan">원본 재확인 필요</option>
            </select>
          </label>
          <label className="field"><span>항목</span>
            <select className="select" value={item} onChange={(e) => setItem(e.target.value)}><option value="all">전체</option><option value="instructor">강사료</option><option value="textbook_material">교재·재료비</option><option value="admin">수용비</option><option value="other">기타</option></select>
          </label>
          <label className="field"><span>프로그램</span><input className="input" value={program} onChange={(e) => setProgram(e.target.value)} placeholder="이름 일부" style={{ width: 140 }} /></label>
          <Button variant="primary" disabled={monthId === ''} onClick={() => go('recon')} style={{ alignSelf: 'flex-end' }}>Excel 저장…</Button>
        </div>
      </Card>
      <Card title="B. 자유수강권 사용액 현황">
        <div className="row"><span className="muted">대상자 전원 · 월별 사용액 · 누적 · 잔액 · 수익자 전환액 · 방과후이용권 사용액(별도)</span><Button variant="primary" onClick={() => go('usage')}>Excel 저장…</Button></div>
      </Card>
      <Card title="C. 자유수강권 소급 환불 현황">
        <div className="row">
          <label className="field"><span>처리 상태</span>
            <select className="select" value={refundStatus} onChange={(e) => setRefundStatus(e.target.value)}><option value="all">전체</option><option value="open">미처리·처리 예정·일부 완료</option><option value="none">미처리</option><option value="planned">처리 예정</option><option value="partial">일부 완료</option><option value="complete">처리 완료</option><option value="cancelled">취소</option></select>
          </label>
          <label className="field"><span>기존 재원</span>
            <select className="select" value={refundFund} onChange={(e) => setRefundFund(e.target.value)}><option value="all">전체</option><option value="초3 초과금">초3 초과금</option><option value="일반 수익자">일반 수익자</option></select>
          </label>
          <Button variant="primary" onClick={() => go('refunds')} style={{ alignSelf: 'flex-end' }}>Excel 저장…</Button>
        </div>
      </Card>
      <Card title="D. 회계 무결성 검사 결과">
        <div className="row"><span className="muted">지금 상태로 전체 검사를 실행해 결과와 항목 요약 두 시트로 저장합니다.</span><Button variant="primary" onClick={() => go('integrity')}>Excel 저장…</Button></div>
      </Card>
    </>
  )
}

/* ── 작업 이력 ─────────────────────────────────────────────── */

const KIND_LABEL: Record<string, string> = { backup: '백업', restore: '복원', integrity: '무결성 검사', export: '내보내기', cleanup: '백업 정리' }
const STATUS_CLS: Record<string, string> = { ok: 'badge badge--ok', failed: 'badge badge--bad', blocked: 'badge badge--soon' }

function History() {
  const h = useQuery({ queryKey: ['opsHistory'], queryFn: () => api.opsHistory(300) })
  const [kind, setKind] = useState('all')
  const rows = (h.data ?? []).filter((r) => kind === 'all' || r.kind === kind)
  return (
    <Card title="작업 이력 (자료 폴더의 ops-history.jsonl — DB 밖에 남아 복원해도 유지)" right={<select className="select" value={kind} onChange={(e) => setKind(e.target.value)}><option value="all">전체</option><option value="backup">백업</option><option value="restore">복원</option><option value="integrity">무결성 검사</option><option value="export">내보내기</option><option value="cleanup">백업 정리</option></select>}>
      {h.error && <Notice kind="bad">{errorMessage(h.error)}</Notice>}
      {h.data && rows.length === 0 && <Empty title="작업 이력이 없습니다." />}
      {rows.length > 0 && (
        <div className="tableWrap">
          <table className="table">
            <thead><tr><th>시각</th><th>작업</th><th className="center">결과</th><th>내용</th><th>파일</th></tr></thead>
            <tbody>{rows.map((r, i) => <tr key={i}><td className="muted">{shortDateTime(r.at)}</td><td>{KIND_LABEL[r.kind] ?? r.kind}</td><td className="center"><span className={STATUS_CLS[r.status] ?? 'badge badge--muted'}>{r.status === 'ok' ? '정상' : r.status === 'failed' ? '실패' : '차단'}</span></td><td className="wrap">{r.summary}</td><td className="wrap" style={{ fontSize: 12 }}>{r.file ?? ''}</td></tr>)}</tbody>
          </table>
        </div>
      )}
    </Card>
  )
}
