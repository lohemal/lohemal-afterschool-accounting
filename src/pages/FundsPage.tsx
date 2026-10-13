import { useQuery } from '@tanstack/react-query'
import { Link } from 'react-router-dom'

import { Card, Notice, Page, Soon } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'

export function FundsPage() {
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })

  return (
    <Page title="재원별 관리" sub="재원(수익자·자유수강권·초3 지원금 등)별 징수·지출 집계와 재원 간 이동(과목경정)을 다룹니다.">
      <Soon phase="P2 · P4">
        재원별 월 집계, 자유수강권 소급 환불, 재원 간 이동 등록은 아직 구현되지 않았습니다. 재원 이동은 두 줄의 합이 0 인 쌍으로만
        저장되도록 자료 모델에 못 박혀 있습니다.
      </Soon>

      <Card title="재원 유형" right={<Link to="/settings">이름 바꾸기·추가는 설정에서</Link>}>
        {cat.error && <Notice kind="bad">{errorMessage(cat.error)}</Notice>}
        {cat.data && (
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>재원</th>
                  <th>코드</th>
                  <th>상위 재원</th>
                  <th className="center">구분</th>
                  <th className="center">사용</th>
                </tr>
              </thead>
              <tbody>
                {cat.data.fundTypes.map((f) => (
                  <tr key={f.id}>
                    <td>{f.name}</td>
                    <td className="mono muted">{f.code}</td>
                    <td className="muted">{cat.data.fundTypes.find((p) => p.id === f.parentId)?.name ?? '—'}</td>
                    <td className="center">
                      <span className={f.isBuiltin ? 'badge badge--navy' : 'badge badge--muted'}>{f.isBuiltin ? '기본' : '사용자'}</span>
                    </td>
                    <td className="center">{f.isActive ? '사용' : <span className="muted">사용 안 함</span>}</td>
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
