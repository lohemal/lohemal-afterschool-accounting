import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { AllocationPanel } from '@/components/AllocationPanel'
import { Modal } from '@/components/Modal'
import { ReconDetailModal } from '@/components/ReconDetailModal'
import { ReconTable } from '@/components/ReconTable'
import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import { currentYm } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { Flow, ReconRow, Stage } from '@/ipc/types'

const FLOW_LABEL: Record<Flow, string> = { collection: '징수', expenditure: '지출', transfer: '재원 이동' }
const STAGE_LABEL: Record<Stage, string> = { planned: '예정', actual: '실제' }

export function MonthlyPage() {
  const { year } = useApp()
  const qc = useQueryClient()
  const toast = useToast()
  const months = useQuery({
    queryKey: ['months', year?.id],
    queryFn: () => api.monthList(year!.id),
    enabled: !!year,
  })
  const [picked, setPicked] = useState<number | null>(null)
  const [tab, setTab] = useState<'recon' | 'alloc' | 'ledger'>('recon')
  const [closing, setClosing] = useState<'close' | 'reopen' | null>(null)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)
  const [detail, setDetail] = useState<ReconRow | null>(null)

  useEffect(() => {
    if (picked !== null || !months.data?.length) return
    const today = currentYm(new Date())
    const m = months.data.find((x) => x.ym === today) ?? months.data[0]
    setPicked(m.id)
  }, [months.data, picked])

  const recon = useQuery({ queryKey: ['recon', picked], queryFn: () => api.reconMonth(picked!), enabled: picked !== null })
  const summary = useQuery({ queryKey: ['monthSummary', picked], queryFn: () => api.monthSummary(picked!), enabled: picked !== null && tab === 'ledger' })

  async function doClose() {
    if (picked === null || !closing) return
    setBusy(true)
    try {
      if (closing === 'close') await api.monthClose(picked, note)
      else await api.monthReopen(picked, note)
      toast.show(closing === 'close' ? '달을 확정했습니다.' : '달을 재개했습니다.')
      setClosing(null)
      setNote('')
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  if (!year) {
    return (
      <Page title="월별 정산">
        <Notice>학년도가 없습니다. 대시보드나 설정에서 학년도를 먼저 만들어 주세요.</Notice>
      </Page>
    )
  }

  const current = months.data?.find((m) => m.id === picked) ?? null
  const r = recon.data

  return (
    <Page title="월별 정산" sub={`${year.name} · 달을 고르면 프로그램별로 징수 기준액과 지급액을 대조합니다. 차액 = 징수 기준액 − 지급액.`}>
      {months.error && <Notice kind="bad">{errorMessage(months.error)}</Notice>}
      <div className="months">
        {(months.data ?? []).map((m) => (
          <button key={m.id} type="button" className={m.id === picked ? 'month month--on' : 'month'} onClick={() => setPicked(m.id)}>
            <div className="month__label">
              {m.label}
              {m.status === 'closed' ? ' 🔒' : ''}
            </div>
            <div className="month__ym">{m.ym}</div>
            <div className="month__meta">{m.entryCount === 0 ? '자료 없음' : `${won(m.entryCount)}건`}</div>
          </button>
        ))}
      </div>

      {current && (
        <Card
          title={`${current.label} (${current.ym})`}
          right={
            <span className="row" style={{ gap: 6 }}>
              {current.status === 'closed' ? <span className="badge badge--navy">확정</span> : <span className="badge badge--muted">열림</span>}
              {current.changedAfterClose && <span className="badge badge--bad">확정 후 변경</span>}
              {current.status === 'closed' ? <Button size="sm" onClick={() => setClosing('reopen')}>재개…</Button> : <Button size="sm" onClick={() => setClosing('close')}>확정…</Button>}
            </span>
          }
        >
          {r && (
            <div className="row" style={{ marginBottom: 10 }}>
              <div className="stat"><span>전체 징수액 (유효 거래)</span><strong>{won(r.collectionTotal)}</strong></div>
              <div className="stat"><span>전체 지출액 (유효 거래)</span><strong>{won(r.expenditureTotal)}</strong></div>
              <div className="stat"><span>대조 가능 기준액</span><strong>{won(r.comparableBasis)}</strong></div>
              <div className="stat"><span>대조 가능 지급액</span><strong>{won(r.comparableExpense)}</strong></div>
              <div className={`stat ${r.unallocatedTotal ? 'stat--warn' : ''}`}><span>미배분 징수액</span><strong>{won(r.unallocatedTotal)}</strong></div>
              <div className={`stat ${r.mismatchCount ? 'stat--bad' : ''}`}><span>불일치</span><strong>{won(r.mismatchCount)}건</strong></div>
              <div className={`stat ${r.todoCount ? 'stat--warn' : ''}`}><span>확인 필요</span><strong>{won(r.todoCount)}건</strong></div>
            </div>
          )}
          <p className="muted" style={{ margin: '0 0 8px', fontSize: 12 }}>
            전체 징수액과 지출액은 범위가 달라(맞춤형지원 등) 두 값의 차이가 곧 회계 오류는 아닙니다. 대조는 같은 프로그램·재원·항목끼리만 합니다.
          </p>
          <div className="tabs">
            <button type="button" className={tab === 'recon' ? 'tab tab--on' : 'tab'} onClick={() => setTab('recon')}>프로그램별 검증 현황</button>
            <button type="button" className={tab === 'alloc' ? 'tab tab--on' : 'tab'} onClick={() => setTab('alloc')}>징수액 배분{r && r.unallocatedTotal ? ` (미배분 ${won(r.unallocatedTotal)})` : ''}</button>
            <button type="button" className={tab === 'ledger' ? 'tab tab--on' : 'tab'} onClick={() => setTab('ledger')}>거래 합계</button>
          </div>

          {tab === 'recon' && (
            <>
              {recon.isLoading && <span className="spin" />}
              {recon.error && <Notice kind="bad">{errorMessage(recon.error)}</Notice>}
              {r && r.rows.length === 0 && <Empty title="이 달에 귀속된 거래가 없습니다.">자료 업로드에서 징수·지급 파일을 올리면 프로그램별 대조가 여기에 나타납니다.</Empty>}
              {r && r.rows.length > 0 && <ReconTable rows={r.rows} onOpen={setDetail} />}
              <p className="muted" style={{ margin: '8px 0 0', fontSize: 12 }}>
                정상 = 비교 가능한 재원이 모두 0원 차액. 자료가 없는 쪽을 0원으로 보지 않으며, 배분·매핑이 끝나지 않은 항목은 차액이 0이어도 정상으로 표시하지 않습니다. 수용비·기타는 대응 지출 자료가 없어 검증 불가입니다.
              </p>
            </>
          )}
          {tab === 'alloc' && <AllocationPanel fiscalMonthId={current.id} yearId={year.id} closed={current.status === 'closed'} />}
          {tab === 'ledger' && (
            <>
              {summary.isLoading && <span className="spin" />}
              {summary.data && summary.data.rows.length === 0 && <Empty title="이 달에 귀속된 거래가 없습니다." />}
              {summary.data && summary.data.rows.length > 0 && (
                <div className="tableWrap">
                  <table className="table">
                    <thead>
                      <tr>
                        <th>프로그램</th>
                        <th>재원</th>
                        <th>항목</th>
                        <th className="center">흐름</th>
                        <th className="center">단계</th>
                        <th className="num">건수</th>
                        <th className="num">금액</th>
                      </tr>
                    </thead>
                    <tbody>
                      {summary.data.rows.map((x, i) => (
                        <tr key={i} className={x.programName === '미배분' || x.fundTypeName === '미확정' ? 'row--check' : ''}>
                          <td>{x.programName}</td>
                          <td>{x.fundTypeName}</td>
                          <td>{x.amountItemName}</td>
                          <td className="center">{FLOW_LABEL[x.flow]}</td>
                          <td className="center">{STAGE_LABEL[x.stage]}</td>
                          <td className="num">{won(x.entryCount)}</td>
                          <td className="num">{won(x.total)}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </Card>
      )}

      {detail && current && <ReconDetailModal fiscalMonthId={current.id} row={detail} onClose={() => setDetail(null)} />}

      {closing && current && (
        <Modal
          title={closing === 'close' ? `${current.label} 확정` : `${current.label} 재개`}
          onClose={() => setClosing(null)}
          footer={
            <>
              <Button onClick={() => setClosing(null)}>취소</Button>
              <Button variant="primary" disabled={busy || (closing === 'reopen' && !note.trim())} onClick={doClose}>
                {closing === 'close' ? '확정' : '재개'}
              </Button>
            </>
          }
        >
          {closing === 'close' ? (
            <p style={{ marginTop: 0 }}>확정하면 이 달의 합계가 스냅샷으로 남고, 재개하기 전에는 이 달의 업로드 자료와 배분을 바꿀 수 없습니다. 소급 거래가 들어오면 '확정 후 변경'으로 표시됩니다.</p>
          ) : (
            <p style={{ marginTop: 0 }}>재개하면 이 달의 자료를 다시 바꿀 수 있습니다. 재개 사유는 변경 이력에 남습니다.</p>
          )}
          <div className="row">
            <span className="label">{closing === 'close' ? '메모' : '재개 사유'}</span>
            <input className="input" style={{ flex: 1 }} value={note} onChange={(e) => setNote(e.target.value)} />
          </div>
        </Modal>
      )}
    </Page>
  )
}
