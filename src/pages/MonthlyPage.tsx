import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Card, Empty, Notice, Page, Soon, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import { currentYm } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { Flow, Stage } from '@/ipc/types'

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
  const [closing, setClosing] = useState<'close' | 'reopen' | null>(null)
  const [note, setNote] = useState('')
  const [busy, setBusy] = useState(false)

  // 처음 열면 오늘이 속한 달(학년도 안에 있으면)을 고른다.
  useEffect(() => {
    if (picked !== null || !months.data?.length) return
    const today = currentYm(new Date())
    const m = months.data.find((x) => x.ym === today) ?? months.data[0]
    setPicked(m.id)
  }, [months.data, picked])

  const summary = useQuery({
    queryKey: ['monthSummary', picked],
    queryFn: () => api.monthSummary(picked!),
    enabled: picked !== null,
  })

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

  return (
    <Page title="월별 정산" sub={`${year.name} · 달을 고르면 그 달에 귀속된 거래의 합계를 프로그램·재원·항목별로 보여줍니다.`}>
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
              {current.status === 'closed' ? (
                <Button size="sm" onClick={() => setClosing('reopen')}>재개…</Button>
              ) : (
                <Button size="sm" onClick={() => setClosing('close')}>확정…</Button>
              )}
            </span>
          }
        >
          {summary.isLoading && <span className="spin" />}
          {summary.error && <Notice kind="bad">{errorMessage(summary.error)}</Notice>}
          {summary.data && summary.data.rows.length === 0 && (
            <Empty title="이 달에 귀속된 거래가 없습니다.">자료 업로드에서 최초 징수·추가징수·환불·강사료·교재재료비 파일을 올리면 거래가 여기에 쌓입니다.</Empty>
          )}
          {summary.data && summary.data.rows.length > 0 && (
            <>
              <div className="grid grid--kpi" style={{ marginBottom: 12 }}>
                <Stat label="징수 합계" value={summary.data.collectionTotal} />
                <Stat label="지출 합계" value={summary.data.expenditureTotal} />
                <Stat label="재원 이동 잔액(0 이어야 함)" value={summary.data.transferBalance} />
                <Stat label="거래 건수" value={summary.data.rows.reduce((s, r) => s + r.entryCount, 0)} unit="건" />
              </div>
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
                    {summary.data.rows.map((r, i) => (
                      <tr key={i} className={r.programName === '미배분' || r.fundTypeName === '미확정' ? 'row--check' : ''}>
                        <td>{r.programName}</td>
                        <td>{r.fundTypeName}</td>
                        <td>{r.amountItemName}</td>
                        <td className="center">{FLOW_LABEL[r.flow]}</td>
                        <td className="center">{STAGE_LABEL[r.stage]}</td>
                        <td className="num">{won(r.entryCount)}</td>
                        <td className="num">{won(r.total)}</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
              <p className="muted" style={{ margin: '8px 0 0', fontSize: 12 }}>
                '미배분'은 프로그램 열이 없는 양식(징수명단)에서 온 거래, '미확정'은 재원 열이 없는 양식(1학기 환불·정산)에서 온 거래입니다. 프로그램 연결과 재원 대조는 P2 에서 합니다.
              </p>
            </>
          )}
        </Card>
      )}

      <Soon phase="P2">
        징수(수강생명단)와 지출(강사료·교재재료비)을 프로그램·재원별로 맞춰 보고 차액을 표시하는 검증은 아직 없습니다. 수용비처럼 대응 지출 자료가 없는 항목은 '검증 불가'로 표시될 예정입니다.
      </Soon>

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
            <p style={{ marginTop: 0 }}>확정하면 이 달의 합계가 스냅샷으로 남고, 재개하기 전에는 이 달의 업로드 자료를 교체·취소·복원할 수 없습니다. 소급 거래가 들어오면 '확정 후 변경'으로 표시됩니다.</p>
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

function Stat({ label, value, unit = '원' }: { label: string; value: number; unit?: string }) {
  return (
    <div className="kpi">
      <div className="kpi__label">{label}</div>
      <div className="kpi__value">
        {won(value)}
        <small>{unit}</small>
      </div>
    </div>
  )
}
