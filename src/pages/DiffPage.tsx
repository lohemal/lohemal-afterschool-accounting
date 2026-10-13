import { useQuery } from '@tanstack/react-query'
import { useEffect, useMemo, useState } from 'react'

import { ReconDetailModal } from '@/components/ReconDetailModal'
import { ReconTable } from '@/components/ReconTable'
import { Card, Empty, Notice, Page } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'
import { currentYm } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { ReconRow, ReconStatus } from '@/ipc/types'

const STATUS_OPTIONS: [ReconStatus | 'all' | 'attention', string][] = [
  ['attention', '확인이 필요한 것만 (매핑 · 배분 · 재원 미확정 · 차액 · 자료 확인)'],
  ['all', '전체'],
  ['link_pending', '매핑 확인 필요 (세부반 → 통합 프로그램)'],
  ['unallocated', '배분 미확정'],
  ['unmapped', '재원 매핑 미확정'],
  ['orphan', '원본 재확인 필요'],
  ['mismatch', '차액 확인 필요'],
  ['missing', '자료 확인 필요'],
  ['group_only', '그룹으로 검증'],
  ['ok', '정상'],
  ['unverifiable', '검증 불가'],
]

const RANK: Record<ReconStatus, number> = { link_pending: 0, unallocated: 1, unmapped: 2, orphan: 2, mismatch: 3, missing: 4, group_only: 5, ok: 6, unverifiable: 7 }

export function DiffPage() {
  const { year } = useApp()
  const months = useQuery({ queryKey: ['months', year?.id], queryFn: () => api.monthList(year!.id), enabled: !!year })
  const [picked, setPicked] = useState<number | ''>('')
  const [status, setStatus] = useState<string>('attention')
  const [item, setItem] = useState<string>('all')
  const [noteFilter, setNoteFilter] = useState<string>('all')
  const [program, setProgram] = useState('')
  const [sort, setSort] = useState<'priority' | 'abs'>('priority')
  const [detail, setDetail] = useState<ReconRow | null>(null)

  useEffect(() => {
    if (picked !== '' || !months.data?.length) return
    const today = currentYm(new Date())
    setPicked((months.data.find((m) => m.ym === today) ?? months.data[0]).id)
  }, [months.data, picked])

  const recon = useQuery({ queryKey: ['recon', picked], queryFn: () => api.reconMonth(picked as number), enabled: picked !== '' })

  const rows = useMemo(() => {
    const all = recon.data?.rows ?? []
    let f = all.filter((r) =>
      status === 'all' ? true : status === 'attention' ? ['link_pending', 'unallocated', 'unmapped', 'orphan', 'mismatch', 'missing'].includes(r.status) : r.status === status,
    )
    if (item !== 'all') f = f.filter((r) => r.itemSet === item)
    if (noteFilter !== 'all') f = f.filter((r) => (noteFilter === 'recheck' ? r.needsRecheck : noteFilter === 'none' ? r.noteStatus === 'none' : r.noteStatus === noteFilter))
    if (program.trim()) f = f.filter((r) => r.unitName.includes(program.trim()))
    f = [...f]
    if (sort === 'abs') f.sort((a, b) => Math.abs(b.diff) - Math.abs(a.diff))
    else f.sort((a, b) => RANK[a.status] - RANK[b.status] || a.unitName.localeCompare(b.unitName, 'ko'))
    return f
  }, [recon.data, status, item, noteFilter, program, sort])

  if (!year) {
    return (
      <Page title="차액 분석">
        <Notice>학년도가 없습니다.</Notice>
      </Page>
    )
  }
  const r = recon.data
  return (
    <Page title="차액 분석" sub="월별·프로그램별·재원별로 징수 기준액과 지급액이 맞지 않는 곳을 찾고, 관련 거래와 원본 엑셀 행까지 되짚습니다. 차액을 0으로 맞추는 기능은 없습니다.">
      <Card title="필터">
        <div className="row">
          <label className="field"><span>월</span>
            <select className="select" value={picked} onChange={(e) => setPicked(Number(e.target.value))}>
              {(months.data ?? []).map((m) => <option key={m.id} value={m.id}>{m.label} ({m.ym})</option>)}
            </select>
          </label>
          <label className="field"><span>검증 상태</span>
            <select className="select" value={status} onChange={(e) => setStatus(e.target.value)}>
              {STATUS_OPTIONS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
            </select>
          </label>
          <label className="field"><span>항목</span>
            <select className="select" value={item} onChange={(e) => setItem(e.target.value)}>
              <option value="all">전체</option>
              <option value="instructor">강사료</option>
              <option value="textbook_material">교재·재료비</option>
              <option value="admin">수용비</option>
              <option value="other">기타</option>
            </select>
          </label>
          <label className="field"><span>확인 상태</span>
            <select className="select" value={noteFilter} onChange={(e) => setNoteFilter(e.target.value)}>
              <option value="all">전체</option>
              <option value="recheck">재확인 필요</option>
              <option value="none">기록 없음</option>
              <option value="todo">확인 필요</option>
              <option value="investigating">확인 중</option>
              <option value="explained">사유 확인</option>
            </select>
          </label>
          <label className="field"><span>프로그램</span>
            <input className="input" placeholder="이름 일부" value={program} onChange={(e) => setProgram(e.target.value)} style={{ width: 160 }} />
          </label>
          <label className="field"><span>정렬</span>
            <select className="select" value={sort} onChange={(e) => setSort(e.target.value as 'priority' | 'abs')}>
              <option value="priority">우선순위 (매핑 확인 → 배분 미확정 → 재원 미확정 → 차액 확인 → 자료 확인 → 정상)</option>
              <option value="abs">차액 절댓값 큰 순</option>
            </select>
          </label>
        </div>
      </Card>
      <Card title="결과" right={r ? <span className="muted">매핑 확인 {won(r.linkPendingCount)}건 · 차액 확인 {won(r.mismatchCount)}건 · 미배분 {won(r.unallocatedTotal)} · 확인 필요 {won(r.todoCount)}건</span> : undefined}>
        {recon.isLoading && <span className="spin" />}
        {recon.error && <Notice kind="bad">{errorMessage(recon.error)}</Notice>}
        {r && rows.length === 0 && <Empty title="조건에 맞는 항목이 없습니다." />}
        {r && rows.length > 0 && <ReconTable rows={rows} onOpen={setDetail} />}
      </Card>
      {detail && picked !== '' && <ReconDetailModal fiscalMonthId={picked} row={detail} onClose={() => setDetail(null)} />}
    </Page>
  )
}
