import { useQuery } from '@tanstack/react-query'

import { Card, Empty, Notice, Page } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { shortDateTime } from '@/lib/format'

export function HistoryPage() {
  const log = useQuery({ queryKey: ['audit'], queryFn: () => api.auditList(300) })

  return (
    <Page title="변경 이력" sub="학년도·설정·업로드·거래 정정 등 자료가 바뀐 기록입니다. 개인정보와 금액은 적지 않습니다.">
      <Card title="최근 300건">
        {log.isLoading && <span className="spin" />}
        {log.error && <Notice kind="bad">{errorMessage(log.error)}</Notice>}
        {log.data && log.data.length === 0 && <Empty title="아직 기록이 없습니다." />}
        {log.data && log.data.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>시각</th>
                  <th>종류</th>
                  <th>대상</th>
                  <th>내용</th>
                </tr>
              </thead>
              <tbody>
                {log.data.map((l) => (
                  <tr key={l.id}>
                    <td className="muted">{shortDateTime(l.at)}</td>
                    <td className="mono">{l.kind}</td>
                    <td className="muted">
                      {l.targetTable}
                      {l.targetId !== null ? ` #${l.targetId}` : ''}
                    </td>
                    <td style={{ whiteSpace: 'normal' }}>{l.summary}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>
    </Page>
  )
}
