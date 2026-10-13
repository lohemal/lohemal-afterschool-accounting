import { useQuery } from '@tanstack/react-query'
import { useEffect, useState } from 'react'

import { Card, Empty, Notice, Page, Soon } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import { currentYm } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { Flow, Stage } from '@/ipc/types'

const FLOW_LABEL: Record<Flow, string> = { collection: '징수', expenditure: '지출', transfer: '재원 이동' }
const STAGE_LABEL: Record<Stage, string> = { planned: '예정', actual: '실제' }

export function MonthlyPage() {
  const { year } = useApp()
  const months = useQuery({
    queryKey: ['months', year?.id],
    queryFn: () => api.monthList(year!.id),
    enabled: !!year,
  })
  const [picked, setPicked] = useState<number | null>(null)

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
          <button
            key={m.id}
            type="button"
            className={m.id === picked ? 'month month--on' : 'month'}
            onClick={() => setPicked(m.id)}
          >
            <div className="month__label">{m.label}</div>
            <div className="month__ym">{m.ym}</div>
            <div className="month__meta">{m.entryCount === 0 ? '자료 없음' : `${won(m.entryCount)}건`}</div>
          </button>
        ))}
      </div>

      {current && (
        <Card
          title={`${current.label} (${current.ym})`}
          right={
            <>
              {current.status === 'closed' ? <span className="badge badge--navy">확정</span> : <span className="badge badge--muted">열림</span>}
              {current.changedAfterClose && <span className="badge badge--bad" style={{ marginLeft: 6 }}>확정 후 변경</span>}
            </>
          }
        >
          {summary.isLoading && <span className="spin" />}
          {summary.error && <Notice kind="bad">{errorMessage(summary.error)}</Notice>}
          {summary.data && summary.data.rows.length === 0 && (
            <Empty title="이 달에 귀속된 거래가 없습니다.">
              자료 업로드(P1)가 구현되면 최초 징수·추가징수·환불·강사료·교재재료비 거래가 여기에 쌓입니다.
            </Empty>
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
                      <tr key={i}>
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
            </>
          )}
        </Card>
      )}

      <Soon phase="P2">
        징수(수강생명단)와 지출(강사료·교재재료비)을 프로그램·재원별로 맞춰 보고 차액을 표시하는 검증은 아직 없습니다. 수용비처럼
        대응 지출 자료가 없는 항목은 '검증 불가'로 표시될 예정입니다. 월 확정·재개 기능은 P3 에서 붙습니다.
      </Soon>
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
