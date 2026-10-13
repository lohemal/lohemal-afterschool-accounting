import { useQuery, useQueryClient } from '@tanstack/react-query'
import { open } from '@tauri-apps/plugin-dialog'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { ImpactModal } from '@/components/ImpactModal'
import { Modal } from '@/components/Modal'
import { ProgramMapper } from '@/components/ProgramMapper'
import { RowsTable } from '@/components/RowsTable'
import { UploadDetailModal } from '@/components/UploadDetailModal'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { bytes, shortDateTime, won } from '@/lib/format'
import { useApp } from '@/lib/useApp'
import type { DataKind, Inspect, Preview, PreviewRequest, RowOverride, UploadFile } from '@/ipc/types'

const KIND_LABEL: Record<DataKind, string> = {
  initial_collect: '최초 징수 (원장 반영)',
  extra_refund: '추가징수·환불 (원장 반영)',
  instructor_pay: '강사료 지급 (원장 반영)',
  vendor_pay: '교재·재료비 지급 (원장 반영)',
  roster_link: '프로그램 연결·검증 (원장 반영 안 함)',
  snapshot: '참고 스냅샷 (원장 반영 안 함)',
}

const FORMAT_KINDS: Record<string, DataKind[]> = {
  collect_initial: ['initial_collect', 'snapshot'],
  roster_manager: ['initial_collect', 'snapshot', 'roster_link'],
  settle_s1: ['roster_link', 'initial_collect', 'snapshot'],
  refund_extra_s1: ['extra_refund', 'snapshot'],
  instructor_pay: ['instructor_pay'],
  vendor_pay: ['vendor_pay'],
  roster_s1: ['roster_link', 'snapshot'],
}

const ALL_FORMATS: [string, string][] = [
  ['collect_initial', '징수명단(기안용)'],
  ['roster_manager', '통합 매니저 수강생명단'],
  ['settle_s1', '1학기 정산 명단(징수·환불·추가징수)'],
  ['refund_extra_s1', '환불 및 추가징수 명단'],
  ['instructor_pay', '강사료 지급 내역'],
  ['vendor_pay', '교재·재료비 지급업체'],
  ['roster_s1', '1학기 수강생 명단'],
]

export function UploadPage() {
  const { year } = useApp()
  const [tab, setTab] = useState<'new' | 'history'>('new')
  const [replaceTarget, setReplaceTarget] = useState<UploadFile | null>(null)

  if (!year) {
    return (
      <Page title="자료 업로드">
        <Notice>학년도가 없습니다. 대시보드나 설정에서 학년도를 먼저 만들어 주세요.</Notice>
      </Page>
    )
  }
  return (
    <Page title="자료 업로드" sub="엑셀 파일을 올려 원본 행을 보존하고, 미리보기와 확인을 거쳐 회계 거래로 반영합니다. 잘못 올린 파일은 파일 단위로 되돌릴 수 있습니다.">
      <div className="tabs">
        <button type="button" className={tab === 'new' ? 'tab tab--on' : 'tab'} onClick={() => setTab('new')}>
          새 업로드
        </button>
        <button type="button" className={tab === 'history' ? 'tab tab--on' : 'tab'} onClick={() => setTab('history')}>
          업로드 이력 · 되돌리기
        </button>
      </div>
      {tab === 'new' ? (
        <NewUpload
          yearId={year.id}
          replaceTarget={replaceTarget}
          onClearReplace={() => setReplaceTarget(null)}
          onDone={() => {
            setReplaceTarget(null)
            setTab('history')
          }}
        />
      ) : (
        <History
          yearId={year.id}
          onReplace={(f) => {
            setReplaceTarget(f)
            setTab('new')
          }}
        />
      )}
    </Page>
  )
}

/* ── 새 업로드 ───────────────────────────────────────────────────────── */

function NewUpload({
  yearId,
  replaceTarget,
  onClearReplace,
  onDone,
}: {
  yearId: number
  replaceTarget: UploadFile | null
  onClearReplace: () => void
  onDone: () => void
}) {
  const toast = useToast()
  const qc = useQueryClient()
  const months = useQuery({ queryKey: ['months', yearId], queryFn: () => api.monthList(yearId) })
  const [inspect, setInspect] = useState<Inspect | null>(null)
  const [format, setFormat] = useState('')
  const [kind, setKind] = useState<DataKind | ''>('')
  const [monthId, setMonthId] = useState<number | ''>('')
  const [overrides, setOverrides] = useState<RowOverride[]>([])
  const [preview, setPreview] = useState<Preview | null>(null)
  const [previewing, setPreviewing] = useState(false)
  const [previewError, setPreviewError] = useState<string | null>(null)
  const [confirming, setConfirming] = useState(false)
  const [busy, setBusy] = useState(false)
  const [note, setNote] = useState('')

  // 교체 대상이 넘어오면 귀속월·자료 유형을 그에 맞춘다.
  useEffect(() => {
    if (replaceTarget) {
      setMonthId(replaceTarget.fiscalMonthId ?? '')
      setKind(replaceTarget.dataKind)
    }
  }, [replaceTarget])

  async function pickFile() {
    const picked = await open({
      multiple: false,
      directory: false,
      filters: [{ name: 'Excel', extensions: ['xlsx', 'xlsm'] }],
    })
    if (!picked || typeof picked !== 'string') return
    try {
      const ins = await api.uploadInspect(yearId, picked)
      setInspect(ins)
      setPreview(null)
      setOverrides([])
      const first = ins.detected[0]
      const fmt = first?.code ?? ''
      setFormat(fmt)
      const kinds = FORMAT_KINDS[fmt] ?? []
      if (!replaceTarget) setKind(kinds[0] ?? '')
      if (!replaceTarget && ins.suggestedMonth && months.data) {
        const m = months.data.find((x) => Number(x.ym.slice(5, 7)) === ins.suggestedMonth)
        if (m) setMonthId(m.id)
      }
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }

  const req: PreviewRequest | null = useMemo(() => {
    if (!inspect || !format || !kind || monthId === '') return null
    return {
      path: inspect.path,
      schoolYearId: yearId,
      fiscalMonthId: monthId,
      formatCode: format,
      dataKind: kind,
      overrides,
      replaceFileId: replaceTarget?.id ?? null,
      note: note || null,
    }
  }, [inspect, format, kind, monthId, overrides, replaceTarget, yearId, note])

  const runPreview = useCallback(async () => {
    if (!req) {
      setPreview(null)
      return
    }
    setPreviewing(true)
    setPreviewError(null)
    try {
      setPreview(await api.uploadPreview(req))
    } catch (e) {
      setPreview(null)
      setPreviewError(errorMessage(e))
    } finally {
      setPreviewing(false)
    }
  }, [req])

  useEffect(() => {
    void runPreview()
  }, [runPreview])

  async function commit() {
    if (!req) return
    setBusy(true)
    try {
      const r = await api.uploadCommit(req)
      toast.show(`'${r.file.fileName}' 을(를) 반영했습니다. 거래 ${won(r.entryCount)}건 · 징수 ${won(r.collectionTotal)} · 지출 ${won(r.expenditureTotal)}`)
      await qc.invalidateQueries()
      setInspect(null)
      setPreview(null)
      setConfirming(false)
      onDone()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  const kinds: DataKind[] = FORMAT_KINDS[format] ?? []
  const blankRows = preview?.rows.filter((r) => r.included === false && r.status === 'check' && r.programText === '' && r.programName === '' && r.entryKind !== '') ?? []
  const blankKeys = [...new Set(blankRows.map((r) => `${r.sheet}\u0000${r.rowNo}`))]

  return (
    <>
      {replaceTarget && (
        <Notice>
          <div>
            <strong>교체 모드</strong> — '{replaceTarget.fileName}' (제{replaceTarget.versionNo}판, {replaceTarget.fiscalMonthLabel} · {replaceTarget.dataKindLabel}) 을(를) 새 파일로 대체합니다. 기존 판의 거래는 취소되고 원본은 보존되며, 이력에서 언제든 되돌릴 수 있습니다.{' '}
            <a href="#" onClick={(e) => { e.preventDefault(); onClearReplace() }}>
              교체 모드 해제
            </a>
          </div>
        </Notice>
      )}
      <Card title="1. 파일 · 귀속월 · 자료 유형">
        <div className="filebox">
          <Button variant="primary" onClick={pickFile}>
            파일 선택…
          </Button>
          {inspect ? (
            <div>
              <strong>{inspect.fileName}</strong> <span className="muted">· {bytes(inspect.byteSize)} · 시트 {inspect.sheets.map((s) => `${s.name}(${s.maxRow}행)`).join(', ')}</span>
              <div className="muted" style={{ fontSize: 12 }}>
                {inspect.detected.length === 0
                  ? '양식을 자동으로 알아내지 못했습니다. 아래에서 직접 골라 주세요.'
                  : `인식된 양식: ${inspect.detected.map((d) => `${d.label}${d.confidence < 100 ? ' (확인 권장)' : ''}`).join(' / ')}`}
                {inspect.suggestedMonth ? ` · 제목에서 ${inspect.suggestedMonth}월로 추정 (귀속월은 직접 확인하세요)` : ''}
              </div>
            </div>
          ) : (
            <span className="muted">xlsx · xlsm 파일을 고르면 시트 구조와 머리글로 양식을 알아냅니다. 파일 이름은 보지 않습니다.</span>
          )}
        </div>
        {inspect?.duplicateOf && (
          <Notice kind="bad">
            같은 내용의 파일이 이미 등록되어 있습니다: '{inspect.duplicateOf.fileName}' (제{inspect.duplicateOf.versionNo}판 · {inspect.duplicateOf.statusLabel}). 다시 반영할 수 없습니다.
          </Notice>
        )}
        <div className="upload__top" style={{ marginTop: 12 }}>
          <label className="field">
            <span>양식</span>
            <select className="select" value={format} onChange={(e) => { setFormat(e.target.value); setKind((FORMAT_KINDS[e.target.value] ?? [])[0] ?? '') }} disabled={!inspect}>
              <option value="">— 선택 —</option>
              {ALL_FORMATS.map(([c, l]) => (
                <option key={c} value={c}>
                  {l}
                  {inspect?.detected.some((d) => d.code === c) ? ' ✓' : ''}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>귀속월</span>
            <select className="select" value={monthId} onChange={(e) => setMonthId(e.target.value === '' ? '' : Number(e.target.value))} disabled={!!replaceTarget}>
              <option value="">— 선택 —</option>
              {(months.data ?? []).map((m) => (
                <option key={m.id} value={m.id}>
                  {m.label} ({m.ym}){m.status === 'closed' ? ' · 확정됨' : ''}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>자료 유형</span>
            <select className="select" value={kind} onChange={(e) => setKind(e.target.value as DataKind)} disabled={!format || !!replaceTarget}>
              <option value="">— 선택 —</option>
              {kinds.map((k) => (
                <option key={k} value={k}>
                  {KIND_LABEL[k]}
                </option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>메모 (선택)</span>
            <input className="input" value={note} onChange={(e) => setNote(e.target.value)} placeholder="예: 9월 1차 징수" />
          </label>
        </div>
      </Card>

      {previewing && (
        <div className="card">
          <span className="spin" /> <span className="muted">미리보기 만드는 중…</span>
        </div>
      )}
      {previewError && <Notice kind="bad">{previewError}</Notice>}

      {preview && !previewing && (
        <>
          <Card title="2. 검사 결과 · 매핑">
            {preview.blockReasons.map((b) => (
              <Notice key={b} kind="bad">
                {b}
                {preview.existingApplied && b.includes('교체') && (
                  <Button size="sm" style={{ marginLeft: 'auto' }} onClick={() => { /* 교체 모드는 이력 탭에서 시작한다 */ toast.show("업로드 이력 탭에서 해당 파일의 '교체' 단추를 눌러 주세요.") }}>
                    교체 방법
                  </Button>
                )}
              </Notice>
            ))}
            {preview.warnings.map((w) => (
              <div key={w} className="notice notice--soon">
                {w}
              </div>
            ))}
            {preview.fileIssues.filter((i) => i.level === 'info').map((i) => (
              <div key={i.message} className="notice">
                {i.message}
              </div>
            ))}
            <div style={{ marginTop: 10 }}>
              <ProgramMapper schoolYearId={yearId} items={preview.unresolvedPrograms} onSaved={() => { void qc.invalidateQueries({ queryKey: ['programs'] }); void runPreview() }} />
            </div>
            {blankKeys.length > 0 && (
              <BlankRowMapper
                yearId={yearId}
                rows={blankKeys.map((k) => {
                  const [sheet, rowNo] = k.split('\u0000')
                  const r = blankRows.find((x) => x.sheet === sheet && x.rowNo === Number(rowNo))!
                  return { sheet, rowNo: Number(rowNo), suggestion: r.programSuggestion, counterparty: r.counterparty, amount: blankRows.filter((x) => x.sheet === sheet && x.rowNo === Number(rowNo)).reduce((s, x) => s + x.amount, 0) }
                })}
                overrides={overrides}
                onChange={setOverrides}
              />
            )}
            {preview.unresolvedFunds.length > 0 && (
              <FundAliasHint items={preview.unresolvedFunds} onSaved={() => void runPreview()} />
            )}
          </Card>

          <Card title="3. 미리보기 · 금액 요약" right={<span className="muted">{preview.formatLabel} · {preview.dataKindLabel}</span>}>
            <div className="row">
              <div className="stat"><span>원본 행</span><strong>{won(preview.summary.dataRowCount)}</strong></div>
              <div className="stat"><span>정규화 행</span><strong>{won(preview.summary.rowCount)}</strong></div>
              <div className="stat"><span>정상</span><strong>{won(preview.summary.okCount)}</strong></div>
              <div className={`stat ${preview.summary.checkCount ? 'stat--warn' : ''}`}><span>확인 필요</span><strong>{won(preview.summary.checkCount)}</strong></div>
              <div className={`stat ${preview.summary.errorCount ? 'stat--bad' : ''}`}><span>오류</span><strong>{won(preview.summary.errorCount)}</strong></div>
              <div className="stat"><span>원장 제외(취소·0원)</span><strong>{won(preview.summary.excludedRowCount)}</strong></div>
              <div className="stat"><span>원장 반영 행</span><strong>{won(preview.summary.includedCount)}</strong></div>
              <div className="stat"><span>징수 합계</span><strong>{won(preview.summary.collectionTotal)}</strong></div>
              <div className="stat"><span>지출 합계</span><strong>{won(preview.summary.expenditureTotal)}</strong></div>
            </div>
            <div className="grid" style={{ gridTemplateColumns: 'repeat(3, minmax(0, 1fr))', marginTop: 12 }}>
              <SumTable title="항목별" lines={preview.summary.byItem} />
              <SumTable title="재원별" lines={preview.summary.byFund} />
              <SumTable title="프로그램별" lines={preview.summary.byProgram} />
            </div>
            <div style={{ marginTop: 12 }}>
              <RowsTable rows={preview.rows} />
            </div>
            <div className="row" style={{ marginTop: 12, justifyContent: 'flex-end' }}>
              <Button variant="primary" disabled={!preview.canCommit || busy} onClick={() => setConfirming(true)}>
                {replaceTarget ? '교체 반영…' : '반영…'}
              </Button>
            </div>
          </Card>
        </>
      )}

      {confirming && preview && (
        <Modal
          title={replaceTarget ? '교체 반영 확인' : '반영 확인'}
          onClose={() => setConfirming(false)}
          footer={
            <>
              <Button onClick={() => setConfirming(false)}>취소</Button>
              <Button variant="primary" disabled={busy} onClick={commit}>
                {busy ? '반영 중…' : '반영'}
              </Button>
            </>
          }
        >
          <p style={{ marginTop: 0 }}>
            '{inspect?.fileName}' 을(를) <strong>{months.data?.find((m) => m.id === monthId)?.label}</strong> · <strong>{preview.dataKindLabel}</strong> 으로 반영합니다.
          </p>
          <div className="row">
            <div className="stat"><span>원장 반영 행</span><strong>{won(preview.summary.includedCount)}</strong></div>
            <div className="stat"><span>징수 합계</span><strong>{won(preview.summary.collectionTotal)}</strong></div>
            <div className="stat"><span>지출 합계</span><strong>{won(preview.summary.expenditureTotal)}</strong></div>
            {preview.summary.checkCount > 0 && <div className="stat stat--warn"><span>확인 필요 행(그대로 반영)</span><strong>{won(preview.summary.checkCount)}</strong></div>}
          </div>
          {replaceTarget && (
            <div className="notice notice--soon" style={{ marginTop: 10 }}>
              기존 제{replaceTarget.versionNo}판의 거래 {won(replaceTarget.entryCount)}건 (징수 {won(replaceTarget.activeCollectionTotal)} · 지출 {won(replaceTarget.activeExpenditureTotal)})은 취소되고, 이 파일이 제{replaceTarget.versionNo + 1}판으로 적용됩니다. 되돌리기는 이력에서 할 수 있습니다.
            </div>
          )}
          <p className="muted" style={{ marginBottom: 0 }}>반영 뒤 원장 합계가 이 미리보기와 다르면 반영은 자동으로 되돌아갑니다. 원본 파일은 앱 자료 폴더에 보관됩니다.</p>
        </Modal>
      )}
    </>
  )
}

function SumTable({ title, lines }: { title: string; lines: { name: string; count: number; total: number }[] }) {
  return (
    <div className="tableWrap">
      <table className="table">
        <thead>
          <tr>
            <th>{title}</th>
            <th className="num">건</th>
            <th className="num">금액</th>
          </tr>
        </thead>
        <tbody>
          {lines.length === 0 && (
            <tr>
              <td colSpan={3} className="muted">
                —
              </td>
            </tr>
          )}
          {lines.map((l) => (
            <tr key={l.name}>
              <td>{l.name}</td>
              <td className="num">{won(l.count)}</td>
              <td className="num">{won(l.total)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

/** 프로그램명이 비어 있고 병합 셀도 아닌 행 — 행마다 프로그램을 지정한다. */
function BlankRowMapper({
  yearId,
  rows,
  overrides,
  onChange,
}: {
  yearId: number
  rows: { sheet: string; rowNo: number; suggestion: string; counterparty: string; amount: number }[]
  overrides: RowOverride[]
  onChange: (o: RowOverride[]) => void
}) {
  const programs = useQuery({ queryKey: ['programs', yearId], queryFn: () => api.programList(yearId) })
  function set(sheet: string, rowNo: number, programId: number | null) {
    const rest = overrides.filter((o) => !(o.sheet === sheet && o.rowNo === rowNo))
    onChange(programId === null ? rest : [...rest, { sheet, rowNo, programId }])
  }
  return (
    <div className="notice notice--soon" style={{ display: 'block', marginTop: 10 }}>
      <strong>프로그램명이 비어 있는 행 {rows.length}개</strong>
      <span className="muted"> — 병합 셀이 아니라 자동으로 잇지 않았습니다. 바로 위 행의 둘째 업체라면 같은 프로그램을 골라 주세요. 원본 엑셀은 바뀌지 않습니다.</span>
      <div className="tableWrap" style={{ marginTop: 6 }}>
        <table className="table">
          <thead>
            <tr>
              <th>시트·행</th>
              <th>업체</th>
              <th className="num">금액</th>
              <th>바로 위 행</th>
              <th>프로그램 지정</th>
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const cur = overrides.find((o) => o.sheet === r.sheet && o.rowNo === r.rowNo)?.programId ?? ''
              return (
                <tr key={`${r.sheet}-${r.rowNo}`}>
                  <td className="muted">{r.sheet} {r.rowNo}</td>
                  <td>{r.counterparty}</td>
                  <td className="num">{won(r.amount)}</td>
                  <td>{r.suggestion || '—'}</td>
                  <td>
                    <select className="select input--sm" value={cur} onChange={(e) => set(r.sheet, r.rowNo, e.target.value === '' ? null : Number(e.target.value))} style={{ minWidth: 220 }}>
                      <option value="">— 지정 안 함 (반영 불가) —</option>
                      {(programs.data ?? []).map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                          {p.name === r.suggestion ? ' (바로 위 행)' : ''}
                        </option>
                      ))}
                    </select>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
    </div>
  )
}

function FundAliasHint({ items, onSaved }: { items: { text: string; count: number }[]; onSaved: () => void }) {
  const toast = useToast()
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })
  const [pick, setPick] = useState<Record<string, number | ''>>({})
  async function save(text: string) {
    const id = pick[text]
    if (!id) return
    try {
      await api.fundAliasSave(id, text)
      toast.show(`재원 별칭 '${text}' 을(를) 저장했습니다.`)
      onSaved()
    } catch (e) {
      toast.show(errorMessage(e), true)
    }
  }
  return (
    <div className="notice" style={{ display: 'block', marginTop: 10 }}>
      <strong>재원을 알 수 없는 표기 {items.length}개</strong>
      <span className="muted"> — 그대로 반영하면 '미확정' 재원으로 남습니다. 한 재원에 해당한다면 여기서 별칭을 저장할 수 있습니다. 두 재원이 섞인 표기('이용권 + 자유수강권')는 추측하지 않고 미확정으로 둡니다.</span>
      <div className="tableWrap" style={{ marginTop: 6 }}>
        <table className="table">
          <tbody>
            {items.map((it) => (
              <tr key={it.text}>
                <td>
                  <strong>{it.text}</strong> <span className="muted">({it.count}행)</span>
                </td>
                <td>
                  <select className="select input--sm" value={pick[it.text] ?? ''} onChange={(e) => setPick({ ...pick, [it.text]: e.target.value === '' ? '' : Number(e.target.value) })}>
                    <option value="">미확정으로 둠</option>
                    {(cat.data?.fundTypes ?? []).filter((f) => f.isActive).map((f) => (
                      <option key={f.id} value={f.id}>
                        {f.name}
                      </option>
                    ))}
                  </select>
                </td>
                <td>
                  <Button size="sm" disabled={!pick[it.text]} onClick={() => save(it.text)}>
                    별칭 저장
                  </Button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  )
}

/* ── 업로드 이력 ─────────────────────────────────────────────────────── */

function History({ yearId, onReplace }: { yearId: number; onReplace: (f: UploadFile) => void }) {
  const qc = useQueryClient()
  const uploads = useQuery({ queryKey: ['uploads', yearId], queryFn: () => api.uploadList(yearId) })
  const [detail, setDetail] = useState<number | null>(null)
  const [impact, setImpact] = useState<{ id: number; action: 'cancel' | 'restore' } | null>(null)

  const STATUS_CLS: Record<string, string> = { applied: 'badge badge--ok', replaced: 'badge badge--muted', cancelled: 'badge badge--bad', registered: 'badge badge--muted', previewed: 'badge badge--navy' }

  return (
    <Card title="업로드한 파일">
      {uploads.error && <Notice kind="bad">{errorMessage(uploads.error)}</Notice>}
      {uploads.data && uploads.data.length === 0 && <Empty title="등록된 파일이 없습니다.">새 업로드 탭에서 파일을 올려 주세요.</Empty>}
      {uploads.data && uploads.data.length > 0 && (
        <div className="tableWrap">
          <table className="table">
            <thead>
              <tr>
                <th>업로드</th>
                <th>파일</th>
                <th>귀속월</th>
                <th>자료 유형</th>
                <th className="center">판</th>
                <th className="center">상태</th>
                <th className="num">유효 거래</th>
                <th className="num">징수</th>
                <th className="num">지출</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {uploads.data.map((u) => (
                <tr key={u.id} className={u.status === 'cancelled' || u.status === 'replaced' ? 'row--off' : ''}>
                  <td className="muted">{shortDateTime(u.uploadedAt)}</td>
                  <td>
                    {u.fileName}
                    {u.summary.checkCount > 0 && <span className="badge badge--soon" style={{ marginLeft: 6 }}>확인 필요 {u.summary.checkCount}</span>}
                  </td>
                  <td>{u.fiscalMonthLabel}</td>
                  <td>{u.dataKindLabel}</td>
                  <td className="center">{u.versionNo}</td>
                  <td className="center">
                    <span className={STATUS_CLS[u.status] ?? 'badge badge--muted'}>{u.statusLabel}</span>
                  </td>
                  <td className="num">{won(u.entryCount)}</td>
                  <td className="num">{won(u.activeCollectionTotal)}</td>
                  <td className="num">{won(u.activeExpenditureTotal)}</td>
                  <td>
                    <div className="row" style={{ gap: 4, flexWrap: 'nowrap' }}>
                      <Button size="sm" onClick={() => setDetail(u.id)}>상세</Button>
                      {u.status === 'applied' && (
                        <>
                          <Button size="sm" onClick={() => onReplace(u)}>교체</Button>
                          <Button size="sm" onClick={() => setImpact({ id: u.id, action: 'cancel' })}>취소</Button>
                        </>
                      )}
                      {(u.status === 'replaced' || u.status === 'cancelled') && (
                        <Button size="sm" onClick={() => setImpact({ id: u.id, action: 'restore' })}>
                          {u.status === 'replaced' ? '이 판으로 복원' : '다시 적용'}
                        </Button>
                      )}
                    </div>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {detail !== null && <UploadDetailModal fileId={detail} onClose={() => setDetail(null)} />}
      {impact && (
        <ImpactModal
          fileId={impact.id}
          action={impact.action}
          onClose={() => setImpact(null)}
          onDone={() => {
            setImpact(null)
            void qc.invalidateQueries()
          }}
        />
      )}
    </Card>
  )
}
