import { Card, Page, Soon } from '@/components/ui'

export function DiffPage() {
  return (
    <Page title="차액 분석" sub="월별·프로그램별·재원별로 징수와 지출이 맞지 않는 곳을 찾아 원본 거래까지 되짚습니다.">
      <Soon phase="P5">
        차액 추출과 원본 추적은 아직 구현되지 않았습니다. 검증 결과가 없을 때 '정상'으로 표시하지 않습니다.
      </Soon>
      <Card title="구현 예정">
        <ul className="list">
          <li>차액이 생긴 프로그램·재원·항목을 자동으로 추려 금액과 함께 표시</li>
          <li>각 차액에서 관련 거래(최초 징수·추가징수·환불·지급)와 원본 파일 행으로 바로 이동</li>
          <li>수용비처럼 대응 지출 자료가 없는 항목은 '검증 불가'로 따로 표시</li>
          <li>기존 엑셀 파일(1학기·2학기)로 회귀 시험</li>
        </ul>
      </Card>
    </Page>
  )
}
