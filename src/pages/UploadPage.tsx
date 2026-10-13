import { useQuery } from '@tanstack/react-query'

import { Card, Empty, Notice, Page, Soon } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { bytes, shortDateTime } from '@/lib/format'
import { useApp } from '@/lib/useApp'
import type { UploadStatus } from '@/ipc/types'

const STATUS: Record<UploadStatus, { label: string; cls: string }> = {
  registered: { label: '등록', cls: 'badge badge--muted' },
  previewed: { label: '미리보기', cls: 'badge badge--navy' },
  applied: { label: '반영됨', cls: 'badge badge--ok' },
  cancelled: { label: '취소', cls: 'badge badge--bad' },
  replaced: { label: '대체됨', cls: 'badge badge--muted' },
}

/** 실제 파일을 분석해 확인한 양식. 열 위치는 P1 에서 코드로 옮긴다. */
const FORMATS = [
  { name: '수강생명단 (방과후 통합 매니저, 2학기)', sheet: '수강생명단', head: '1행', key: '부서·학년·반·번호·이름·지원유형·강사료·수용비·교재비·재료비·합계·상태·변경사유' },
  { name: '강사료 내역', sheet: '강사료내역', head: '3행', key: '부서·강사명·맞춤형지원·3학년 지원금·3학년 수익자·수익자징수·자유수강권·최종강사료합계' },
  { name: '교재·재료비 지급업체', sheet: '(월 이름)', head: '3행', key: '부서명·수익자징수금액·3학년 지원금·3학년 수익자·자유수강권·최종지급금액·업체명 (부서명 공란 행 있음)' },
  { name: '수강생 명단2 (1학기, 환불·추가징수 포함)', sheet: '요약 / 부서별 / 학생별', head: '2행(2단 머리글)', key: '징수·환불·추가징수·합계 × 강사료·수용비·교재비·재료비' },
  { name: '환불 및 추가징수 명단 (1학기)', sheet: '환불 / 추가징수', head: '1행', key: '부서·학년·반·번호·이름·강사료·수용비·교재비·재료비·합계' },
  { name: '징수명단 (기안용)', sheet: '전체 / 수익자 / 방과후이용권 / 자유수강권', head: '3행', key: '학년·반·번호·이름·강사료·수용비·교재비·재료비·합계 (+ 오른쪽 총계 블록)' },
]

export function UploadPage() {
  const { year } = useApp()
  const uploads = useQuery({
    queryKey: ['uploads', year?.id],
    queryFn: () => api.uploadList(year!.id),
    enabled: !!year,
  })

  return (
    <Page title="자료 업로드" sub="엑셀 파일을 올려 원본 행을 보존하고, 미리보기와 확인을 거쳐 회계 거래로 반영합니다.">
      <Soon phase="P1">
        파일 선택·양식 인식·미리보기·중복 감지·반영은 아직 구현되지 않았습니다. 자료 모델(업로드 파일·원본 행·같은 내용 재업로드
        차단)은 준비되어 있습니다.
      </Soon>

      <Card title="등록된 파일">
        {!year && <Notice>학년도를 먼저 만들어 주세요.</Notice>}
        {uploads.error && <Notice kind="bad">{errorMessage(uploads.error)}</Notice>}
        {uploads.data && uploads.data.length === 0 && <Empty title="등록된 파일이 없습니다." />}
        {uploads.data && uploads.data.length > 0 && (
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>파일</th>
                  <th className="center">판</th>
                  <th>양식</th>
                  <th className="center">상태</th>
                  <th className="num">원본 행</th>
                  <th className="num">거래</th>
                  <th className="num">크기</th>
                  <th>등록</th>
                </tr>
              </thead>
              <tbody>
                {uploads.data.map((u) => (
                  <tr key={u.id}>
                    <td>{u.fileName}</td>
                    <td className="center">{u.versionNo}</td>
                    <td>{u.formatCode}</td>
                    <td className="center">
                      <span className={STATUS[u.status].cls}>{STATUS[u.status].label}</span>
                    </td>
                    <td className="num">{u.rowCount}</td>
                    <td className="num">{u.entryCount}</td>
                    <td className="num">{bytes(u.byteSize)}</td>
                    <td className="muted">{shortDateTime(u.uploadedAt)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </Card>

      <Card title="인식 예정 양식 (실제 파일 분석 결과)">
        <div className="tableWrap">
          <table className="table">
            <thead>
              <tr>
                <th>양식</th>
                <th>시트</th>
                <th className="center">머리글</th>
                <th>주요 열</th>
              </tr>
            </thead>
            <tbody>
              {FORMATS.map((f) => (
                <tr key={f.name}>
                  <td>{f.name}</td>
                  <td>{f.sheet}</td>
                  <td className="center">{f.head}</td>
                  <td style={{ whiteSpace: 'normal' }}>{f.key}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Card>
    </Page>
  )
}
