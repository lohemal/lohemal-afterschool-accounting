import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { shortDateTime, won } from '@/lib/format'
import { useApp } from '@/lib/useApp'
import type { LinkItem, LinkPreview, LinkRow } from '@/ipc/types'

type Tab = 'rows' | 'parents' | 'history'
type Filter = 'all' | 'suggested' | 'unlinked' | 'confirmed'

const TABS: [Tab, string][] = [
  ['rows', '세부반 매핑'],
  ['parents', '통합 프로그램별'],
  ['history', '변경 이력'],
]

const FILTERS: [Filter, string][] = [
  ['all', '전체'],
  ['suggested', '추천 미확정만'],
  ['unlinked', '매핑되지 않은 것만 (징수·지급 한쪽만 있음)'],
  ['confirmed', '확정된 세부반만'],
]

interface Pending {
  title: string
  items: LinkItem[]
  rule: 'suggested' | 'manual'
}

/**
 * 프로그램 매핑 — 세부반('로봇과학A') → 통합 프로그램('로봇과학') 연결을 관리한다.
 * 추천은 시스템이, 확정은 사용자가 한다. 확정 전에는 회계 검증에 적용되지 않는다.
 */
export function ProgramLinkPage() {
  const { year } = useApp()
  const qc = useQueryClient()
  const toast = useToast()
  const [tab, setTab] = useState<Tab>('rows')
  const [filter, setFilter] = useState<Filter>('all')
  const [text, setText] = useState('')
  const [pick, setPick] = useState<Record<number, string>>({})
  const [pending, setPending] = useState<Pending | null>(null)
  const [umbrellaName, setUmbrellaName] = useState('')
  const [umbrellaMembers, setUmbrellaMembers] = useState<number[]>([])
  const [busy, setBusy] = useState(false)

  const board = useQuery({ queryKey: ['linkBoard', year?.id], queryFn: () => api.programLinkBoard(year!.id), enabled: !!year })
  const history = useQuery({ queryKey: ['linkHistory', year?.id], queryFn: () => api.programLinkHistory(year!.id), enabled: !!year && tab === 'history' })

  const rows = useMemo(() => {
    const all = board.data?.rows ?? []
    let f = all
    if (filter === 'suggested') f = f.filter((r) => r.status === 'suggested')
    else if (filter === 'unlinked') f = f.filter((r) => r.status !== 'confirmed' && r.role !== 'parent' && !r.groupName && ((r.collectionCount > 0) !== (r.expenseCount > 0)))
    else if (filter === 'confirmed') f = f.filter((r) => r.status === 'confirmed')
    if (text.trim()) f = f.filter((r) => r.name.includes(text.trim()) || (r.parentName ?? '').includes(text.trim()))
    return f
  }, [board.data, filter, text])

  async function refresh() {
    await qc.invalidateQueries({ queryKey: ['linkBoard'] })
    await qc.invalidateQueries({ queryKey: ['linkHistory'] })
    await qc.invalidateQueries({ queryKey: ['programs'] })
    await qc.invalidateQueries({ queryKey: ['recon'] })
    await qc.invalidateQueries({ queryKey: ['reconDetail'] })
  }

  async function createUmbrella() {
    if (!year) return
    setBusy(true)
    try {
      await api.programUmbrellaCreate(year.id, umbrellaName.trim(), umbrellaMembers, '통합 프로그램 직접 생성')
      toast.show(`통합 프로그램 '${umbrellaName.trim()}' 을(를) 만들고 세부반 ${umbrellaMembers.length}개를 연결했습니다.`)
      setUmbrellaName('')
      setUmbrellaMembers([])
      await refresh()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  if (!year) {
    return (
      <Page title="프로그램 매핑">
        <Notice>학년도가 없습니다. 대시보드나 설정에서 학년도를 먼저 만들어 주세요.</Notice>
      </Page>
    )
  }
  const b = board.data
  const parentsForPick = (b?.rows ?? []).filter((r) => r.role !== 'child')
  const suggestedAll: LinkItem[] = (b?.rows ?? []).filter((r) => r.status === 'suggested' && r.suggestion).map((r) => ({ programId: r.programId, parentId: r.suggestion!.parentId }))

  return (
    <Page
      title="프로그램 매핑"
      sub="학생 징수자료는 세부반('로봇과학A'·'로봇과학B') 단위, 강사료·교재재료비 지출은 프로그램('로봇과학') 단위입니다. 세부반을 통합 프로그램에 연결하면 회계 검증은 통합 프로그램 기준으로 징수를 합산해 지급과 대조합니다. 원본 프로그램명과 원장 거래는 바뀌지 않으며, 추천은 사용자가 확정해야 적용됩니다."
    >
      {board.error && <Notice kind="bad">{errorMessage(board.error)}</Notice>}
      {b && (
        <div className="row" style={{ marginBottom: 10 }}>
          <div className={`stat ${b.pendingCount ? 'stat--warn' : ''}`}><span>추천 미확정</span><strong>{won(b.pendingCount)}건</strong></div>
          <div className="stat"><span>확정된 세부반</span><strong>{won(b.confirmedCount)}건</strong></div>
          <div className="stat"><span>통합 프로그램</span><strong>{won(b.parents.length)}개</strong></div>
          <div className="stat"><span>후보 (조건 미충족)</span><strong>{won(b.candidateCount)}건</strong></div>
          <div className="stat"><span>프로그램 전체</span><strong>{won(b.rows.length)}개</strong></div>
        </div>
      )}
      <div className="tabs">
        {TABS.map(([k, l]) => (
          <button key={k} type="button" className={tab === k ? 'tab tab--on' : 'tab'} onClick={() => setTab(k)}>
            {l}
          </button>
        ))}
      </div>

      {tab === 'rows' && (
        <>
          <Card
            title="세부반 → 통합 프로그램"
            right={
              <span className="row" style={{ gap: 6 }}>
                <Button size="sm" variant="primary" disabled={busy || suggestedAll.length === 0} onClick={() => setPending({ title: `추천 ${suggestedAll.length}건 일괄 확정`, items: suggestedAll, rule: 'suggested' })}>
                  추천 전체 확정… ({suggestedAll.length})
                </Button>
              </span>
            }
          >
            <div className="row" style={{ marginBottom: 8 }}>
              <label className="field"><span>보기</span>
                <select className="select input--sm" value={filter} onChange={(e) => setFilter(e.target.value as Filter)}>
                  {FILTERS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select>
              </label>
              <label className="field"><span>프로그램</span>
                <input className="input input--sm" placeholder="이름 일부" value={text} onChange={(e) => setText(e.target.value)} style={{ width: 160 }} />
              </label>
            </div>
            {board.isLoading && <span className="spin" />}
            {b && rows.length === 0 && <Empty title="조건에 맞는 프로그램이 없습니다.">업로드에서 등록한 프로그램이 여기에 나타납니다. 지급 자료가 있는 프로그램과 반 구분만 다른 이름이면 자동으로 추천됩니다.</Empty>}
            {b && rows.length > 0 && (
              <div className="tableWrap">
                <table className="table">
                  <thead>
                    <tr>
                      <th>학생 관리용 세부반 (프로그램)</th>
                      <th>역할</th>
                      <th className="num">징수</th>
                      <th className="num">지급</th>
                      <th>추천 통합 프로그램</th>
                      <th>매핑 상태</th>
                      <th>연결</th>
                      <th />
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((r) => (
                      <LinkRowView
                        key={r.programId}
                        r={r}
                        parents={parentsForPick}
                        picked={pick[r.programId] ?? ''}
                        onPick={(v) => setPick({ ...pick, [r.programId]: v })}
                        busy={busy}
                        onAction={(p) => setPending(p)}
                      />
                    ))}
                  </tbody>
                </table>
              </div>
            )}
            <p className="muted" style={{ margin: '8px 0 0', fontSize: 12 }}>
              추천 기준: 끝의 반 구분(A·B·C·1·2…)을 뗀 이름의 프로그램이 있고 그 프로그램에 지급 자료가 있거나 이미 통합 프로그램일 때. 'ABC'→'AB' 같은 영문 끝 글자, 그룹과 같은 이름, 통합 프로그램이 다시 세부반이 되는 경우는 추천하지 않습니다. 확정하기 전에는 회계 검증에 적용되지 않습니다.
            </p>
          </Card>

          <Card title="통합 프로그램 직접 생성">
            <p className="muted" style={{ margin: '0 0 8px' }}>지급 자료의 프로그램명이 아직 없을 때 통합 프로그램을 먼저 만들고 세부반을 연결합니다. 같은 이름의 프로그램이 이미 있으면 그 프로그램에 연결합니다.</p>
            <div className="row">
              <input className="input input--sm" placeholder="통합 프로그램 이름 (예: 로봇과학)" value={umbrellaName} onChange={(e) => setUmbrellaName(e.target.value)} style={{ width: 220 }} />
              <select className="select input--sm" multiple value={umbrellaMembers.map(String)} onChange={(e) => setUmbrellaMembers([...e.target.selectedOptions].map((o) => Number(o.value)))} style={{ minWidth: 260, height: 96 }}>
                {(b?.rows ?? []).filter((r) => r.role === 'none' && !r.groupName).map((r) => (
                  <option key={r.programId} value={r.programId}>{r.name}{r.collectionCount ? ` (징수 ${r.collectionCount}건)` : ''}</option>
                ))}
              </select>
              <Button size="sm" variant="primary" disabled={busy || !umbrellaName.trim()} onClick={createUmbrella}>만들고 연결 (Ctrl 로 여러 개 선택)</Button>
            </div>
          </Card>
        </>
      )}

      {tab === 'parents' && (
        <Card title="통합 프로그램별 연결 세부반">
          {b && b.parents.length === 0 && <Empty title="아직 통합 프로그램이 없습니다.">세부반 매핑에서 추천을 확정하거나 통합 프로그램을 직접 만들면 여기에 나타납니다.</Empty>}
          {b && b.parents.length > 0 && (
            <div className="tableWrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>통합 프로그램</th>
                    <th>연결된 세부반</th>
                    <th className="num">세부반 징수 합계</th>
                    <th className="num">통합 지급액</th>
                    <th />
                  </tr>
                </thead>
                <tbody>
                  {b.parents.map((p) => (
                    <tr key={p.parentId}>
                      <td><strong>{p.name}</strong></td>
                      <td className="wrap" style={{ whiteSpace: 'normal' }}>
                        <div className="chips">
                          {p.children.map((c) => (
                            <span className="chip" key={c.programId}>
                              {c.name} <span className="muted">({won(c.collectionTotal)})</span>
                              <button type="button" title="연결 해제" disabled={busy} onClick={() => setPending({ title: `'${c.name}' 연결 해제`, items: [{ programId: c.programId, parentId: null }], rule: 'manual' })}>×</button>
                            </span>
                          ))}
                        </div>
                      </td>
                      <td className="num">{won(p.collectionTotal)}</td>
                      <td className="num">{won(p.expenseTotal)}</td>
                      <td className="muted">월별 정산에서 통합 행으로 검증</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      )}

      {tab === 'history' && (
        <Card title="매핑 변경 이력">
          {history.isLoading && <span className="spin" />}
          {history.data && history.data.length === 0 && <Empty title="변경 이력이 없습니다." />}
          {history.data && history.data.length > 0 && (
            <div className="tableWrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>변경 일시</th>
                    <th>대상 세부반</th>
                    <th>이전 통합 프로그램</th>
                    <th>변경 후 통합 프로그램</th>
                    <th>변경 유형</th>
                    <th>근거</th>
                    <th>사용자 확인</th>
                    <th>메모</th>
                  </tr>
                </thead>
                <tbody>
                  {history.data.map((l) => (
                    <tr key={l.id}>
                      <td className="muted">{shortDateTime(l.at)}</td>
                      <td>{l.programName}</td>
                      <td>{l.fromParentName ?? <span className="muted">—</span>}</td>
                      <td>{l.toParentName ?? <span className="muted">—</span>}</td>
                      <td>{l.actionLabel}</td>
                      <td>{l.rule === 'suggested' ? '추천 확정' : '직접 선택'}</td>
                      <td>{l.confirmed ? <span className="badge badge--ok">확인</span> : <span className="badge badge--bad">미확인</span>}</td>
                      <td className="wrap">{l.note}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </Card>
      )}

      {pending && (
        <LinkPreviewModal
          yearId={year.id}
          pending={pending}
          onClose={() => setPending(null)}
          onApplied={async (n) => {
            setPending(null)
            setPick({})
            toast.show(`${n}건을 반영했습니다. 월별 정산의 검증 결과가 다시 계산됩니다.`)
            await refresh()
          }}
        />
      )}
    </Page>
  )
}

function LinkRowView({ r, parents, picked, onPick, busy, onAction }: { r: LinkRow; parents: LinkRow[]; picked: string; onPick: (v: string) => void; busy: boolean; onAction: (p: Pending) => void }) {
  const s = r.suggestion
  const statusCls = r.status === 'confirmed' ? 'badge badge--ok' : r.status === 'suggested' ? 'badge badge--soon' : 'badge badge--muted'
  const canBeChild = r.role !== 'parent' && !r.groupName
  return (
    <tr className={r.status === 'suggested' ? 'row--check' : ''}>
      <td>
        <strong>{r.name}</strong>
        {r.aliases.length > 0 && <span className="muted" style={{ marginLeft: 6, fontSize: 11 }}>({r.aliases.join(' · ')})</span>}
      </td>
      <td>
        {r.role === 'parent' && <span className="badge badge--navy">통합 · 세부반 {r.childNames.length}개</span>}
        {r.role === 'child' && <span className="muted">세부반</span>}
        {r.groupName && <span className="badge badge--muted" style={{ marginLeft: 4 }}>그룹 {r.groupName}</span>}
        {r.role === 'none' && !r.groupName && <span className="muted">—</span>}
      </td>
      <td className="num" title={`${r.collectionCount}건`}>{r.collectionCount ? won(r.collectionTotal) : <span className="muted">없음</span>}</td>
      <td className="num" title={`${r.expenseCount}건`}>{r.expenseCount ? won(r.expenseTotal) : <span className="muted">없음</span>}</td>
      <td className="wrap" style={{ whiteSpace: 'normal' }}>
        {r.status === 'confirmed' && <span>{r.parentName} <span className="muted" style={{ fontSize: 11 }}>· {r.rule === 'suggested' ? '추천 확정' : '직접 선택'} · {shortDateTime(r.linkedAt)}</span></span>}
        {r.status !== 'confirmed' && s && <span>{s.parentName} <span className="muted" style={{ fontSize: 11 }}>— {s.reason}</span></span>}
        {r.status !== 'confirmed' && !s && <span className="muted">—</span>}
      </td>
      <td><span className={statusCls}>{r.statusLabel}</span></td>
      <td>
        {canBeChild && (
          <select className="select input--sm" value={picked} onChange={(e) => onPick(e.target.value)} style={{ minWidth: 180 }}>
            <option value="">통합 프로그램 선택…</option>
            {parents.filter((p) => p.programId !== r.programId).map((p) => (
              <option key={p.programId} value={p.programId}>{p.name}{p.role === 'parent' ? ' (통합)' : p.expenseCount ? ' (지급 있음)' : ''}</option>
            ))}
          </select>
        )}
        {!canBeChild && <span className="muted">{r.role === 'parent' ? '통합 프로그램' : '그룹 구성원'}</span>}
      </td>
      <td>
        <span className="row" style={{ gap: 4 }}>
          {r.status === 'suggested' && s && (
            <Button size="sm" variant="primary" disabled={busy} onClick={() => onAction({ title: `'${r.name}' → '${s.parentName}' 확정`, items: [{ programId: r.programId, parentId: s.parentId }], rule: 'suggested' })}>확정</Button>
          )}
          {canBeChild && picked && (
            <Button size="sm" disabled={busy} onClick={() => onAction({ title: `'${r.name}' 연결 ${r.status === 'confirmed' ? '변경' : ''}`, items: [{ programId: r.programId, parentId: Number(picked) }], rule: 'manual' })}>{r.status === 'confirmed' ? '변경…' : '연결…'}</Button>
          )}
          {r.status === 'confirmed' && (
            <Button size="sm" disabled={busy} onClick={() => onAction({ title: `'${r.name}' 연결 해제`, items: [{ programId: r.programId, parentId: null }], rule: 'manual' })}>해제…</Button>
          )}
        </span>
      </td>
    </tr>
  )
}

/** 적용 전 미리보기 — 대상·영향받는 달(확정된 달은 차단)·경고를 보여 주고 확인을 받는다. */
function LinkPreviewModal({ yearId, pending, onClose, onApplied }: { yearId: number; pending: Pending; onClose: () => void; onApplied: (n: number) => Promise<void> }) {
  const toast = useToast()
  const [pv, setPv] = useState<LinkPreview | null>(null)
  const [err, setErr] = useState<string | null>(null)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  useEffect(() => {
    let alive = true
    api.programLinkPreview(yearId, pending.items).then((p) => alive && setPv(p)).catch((e) => alive && setErr(errorMessage(e)))
    return () => {
      alive = false
    }
  }, [yearId, pending])

  async function apply() {
    setBusy(true)
    try {
      const r = await api.programLinkApply(yearId, pending.items, note, pending.rule)
      await onApplied(r.applied)
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal
      title={pending.title}
      onClose={onClose}
      width={860}
      footer={
        <>
          <Button onClick={onClose}>취소</Button>
          <Button variant="primary" disabled={busy || !pv || pv.blocked || pv.changeCount === 0} onClick={apply}>
            {pv ? `${pv.changeCount}건 반영` : '…'}
          </Button>
        </>
      }
    >
      {err && <Notice kind="bad">{err}</Notice>}
      {!pv && !err && <span className="spin" />}
      {pv && (
        <>
          {pv.blocked && (
            <div className="notice notice--bad" style={{ display: 'block', marginBottom: 8 }}>
              <strong>반영할 수 없습니다.</strong>
              <ul className="list">{pv.blockReasons.map((x) => <li key={x}>{x}</li>)}</ul>
            </div>
          )}
          <div className="tableWrap">
            <table className="table">
              <thead><tr><th>세부반</th><th>이전</th><th>변경 후</th><th>유형</th><th className="num">징수</th><th className="num">지급</th><th>확인</th></tr></thead>
              <tbody>
                {pv.items.map((it) => (
                  <tr key={it.programId} className={it.errors.length ? 'row--error' : it.warnings.length ? 'row--check' : ''}>
                    <td>{it.programName}</td>
                    <td>{it.fromParentName ?? <span className="muted">—</span>}</td>
                    <td>{it.toParentName ?? <span className="muted">— (해제)</span>}</td>
                    <td>{it.actionLabel}</td>
                    <td className="num">{won(it.collectionTotal)}</td>
                    <td className="num">{won(it.expenseTotal)}</td>
                    <td className="wrap" style={{ whiteSpace: 'normal', fontSize: 12 }}>
                      {it.errors.map((x) => <div key={x} style={{ color: 'var(--red-600)' }}>{x}</div>)}
                      {it.warnings.map((x) => <div key={x}>{x}</div>)}
                      {it.errors.length === 0 && it.warnings.length === 0 && <span className="muted">—</span>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {pv.months.length > 0 && (
            <p style={{ margin: '8px 0 0' }}>
              영향받는 달: {pv.months.map((m) => <span key={m.fiscalMonthId} className={m.closed ? 'badge badge--bad' : 'badge badge--navy'} style={{ marginRight: 4 }}>{m.label}{m.closed ? ' (확정됨)' : ''}</span>)}
              <span className="muted" style={{ fontSize: 12 }}>— 이 달들의 프로그램별 검증 결과가 다시 계산됩니다. 원장 거래·감사 이력은 바뀌지 않습니다. 확정된 달은 재개한 뒤에만 바꿀 수 있습니다.</span>
            </p>
          )}
          {pv.months.length === 0 && <p className="muted" style={{ margin: '8px 0 0', fontSize: 12 }}>아직 거래가 없는 세부반이라 지금 달라지는 검증 결과는 없습니다. 이후 업로드부터 통합 프로그램 기준으로 대조됩니다.</p>}
          <div className="row" style={{ marginTop: 10 }}>
            <span className="label">메모</span>
            <input className="input" style={{ flex: 1 }} placeholder="선택 (예: 2학기 세부반 구성 확인)" value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        </>
      )}
    </Modal>
  )
}
