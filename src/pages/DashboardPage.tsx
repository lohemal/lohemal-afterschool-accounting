import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'
import { Link } from 'react-router-dom'

import { Button, Card, Empty, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import { schoolYearOf } from '@/lib/months'
import { useApp } from '@/lib/useApp'

export function DashboardPage() {
  const { boot, year, refresh } = useApp()
  const toast = useToast()
  const [creating, setCreating] = useState(false)
  const [newYear, setNewYear] = useState(schoolYearOf(new Date()))

  const months = useQuery({
    queryKey: ['months', year?.id],
    queryFn: () => api.monthList(year!.id),
    enabled: !!year,
  })

  async function createYear() {
    setCreating(true)
    try {
      await api.yearCreate(newYear)
      await refresh()
      toast.show(`${newYear}학년도를 만들었습니다 (3월 ~ 다음 해 2월).`)
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setCreating(false)
    }
  }

  if (!year) {
    return (
      <Page title="대시보드" sub="방과후학교 회계 자료의 월별·프로그램별·재원별 징수·지출을 대조합니다.">
        <Card title="시작하기 — 학년도 만들기">
          <p className="muted" style={{ margin: '0 0 10px' }}>
            학년도는 3월부터 다음 해 2월까지 12개 회계월로 만들어집니다. 아직 아무 자료도 없습니다.
          </p>
          <div className="row">
            <span className="label">학년도</span>
            <input
              className="input"
              type="number"
              min={2000}
              max={2100}
              value={newYear}
              onChange={(e) => setNewYear(Number(e.target.value))}
              style={{ width: 110 }}
            />
            <span className="muted">학년도</span>
            <Button variant="primary" onClick={createYear} disabled={creating}>
              {creating ? '만드는 중…' : '만들기'}
            </Button>
          </div>
        </Card>
      </Page>
    )
  }

  const c = boot.counts
  const ms = months.data ?? []
  const totalEntries = ms.reduce((s, m) => s + m.entryCount, 0)

  return (
    <Page title="대시보드" sub={`${year.name} · 등록된 자료를 바탕으로 현재 상태를 보여줍니다.`}>
      <div className="grid grid--kpi">
        <Kpi label="프로그램" value={c.programs} unit="개" />
        <Kpi label="업로드 파일(유효)" value={c.uploads} unit="건" />
        <Kpi label="회계 거래(유효)" value={c.entries} unit="건" />
        <Kpi label="학생" value={c.students} unit="명" />
      </div>

      <div style={{ height: 14 }} />

      {totalEntries === 0 ? (
        <Notice>
          <div>
            <strong>등록된 회계 거래가 없습니다.</strong> 검증 결과도 없습니다. 자료 업로드(P1)가 구현되면 수강생명단·강사료·교재재료비
            파일을 올려 거래를 만들 수 있습니다.
          </div>
        </Notice>
      ) : null}

      <Card title="월별 현황" right={<Link to="/monthly">월별 정산으로 →</Link>}>
        {months.isLoading && <span className="spin" />}
        {months.error && <Notice kind="bad">{errorMessage(months.error)}</Notice>}
        {ms.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>월</th>
                  <th>귀속</th>
                  <th className="center">상태</th>
                  <th className="num">거래 건수</th>
                  <th className="num">징수 합계</th>
                  <th className="num">지출 합계</th>
                  <th className="center">검증</th>
                </tr>
              </thead>
              <tbody>
                {ms.map((m) => (
                  <tr key={m.id}>
                    <td>{m.label}</td>
                    <td className="muted">{m.ym}</td>
                    <td className="center">
                      {m.status === 'closed' ? (
                        <span className="badge badge--navy">확정</span>
                      ) : (
                        <span className="badge badge--muted">열림</span>
                      )}
                      {m.changedAfterClose && <span className="badge badge--bad" style={{ marginLeft: 4 }}>확정 후 변경</span>}
                    </td>
                    <td className="num">{m.entryCount === 0 ? <span className="muted">—</span> : won(m.entryCount)}</td>
                    <td className="num">{m.entryCount === 0 ? <span className="muted">자료 없음</span> : won(m.collectionTotal)}</td>
                    <td className="num">{m.entryCount === 0 ? <span className="muted">자료 없음</span> : won(m.expenditureTotal)}</td>
                    <td className="center">
                      {m.entryCount === 0 ? <span className="badge badge--muted">자료 없음</span> : <Link to="/monthly">월별 정산에서 확인</Link>}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        {!months.isLoading && ms.length === 0 && <Empty title="회계월이 없습니다." />}
      </Card>

      <Notice>
        <div>프로그램별 징수·지출 대조와 차액은 <Link to="/monthly">월별 정산</Link>과 <Link to="/diff">차액 분석</Link>에서 봅니다. 이 표는 유효 거래의 합계만 보여 주며, 합계가 같다는 것이 곧 정상을 뜻하지는 않습니다.</div>
      </Notice>
    </Page>
  )
}

function Kpi({ label, value, unit }: { label: string; value: number; unit: string }) {
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
