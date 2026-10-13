import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { Modal } from '@/components/Modal'
import { Button, Notice, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { diff, won } from '@/lib/format'
import type { Impact } from '@/ipc/types'

/** 취소·복원 전 미리보기 — 전후 금액과 영향을 보여 주고 사유를 받은 뒤에만 실행한다. */
export function ImpactModal({
  fileId,
  action,
  onClose,
  onDone,
}: {
  fileId: number
  action: 'cancel' | 'restore'
  onClose: () => void
  onDone: () => void
}) {
  const toast = useToast()
  const q = useQuery({ queryKey: ['impact', fileId, action], queryFn: () => api.uploadImpact(fileId, action) })
  const [reason, setReason] = useState('')
  const [busy, setBusy] = useState(false)
  const title = action === 'cancel' ? '업로드 취소' : '이전 버전으로 복원'

  async function run() {
    setBusy(true)
    try {
      if (action === 'cancel') await api.uploadCancel(fileId, reason)
      else await api.uploadRestore(fileId, reason)
      toast.show(action === 'cancel' ? '업로드를 취소했습니다.' : '이전 버전을 복원했습니다.')
      onDone()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  const imp = q.data
  return (
    <Modal
      title={title}
      onClose={onClose}
      width={720}
      footer={
        <>
          <Button onClick={onClose}>닫기</Button>
          <Button variant="primary" disabled={busy || !imp || imp.blocked.length > 0 || !reason.trim()} onClick={run}>
            {action === 'cancel' ? '취소 실행' : '복원 실행'}
          </Button>
        </>
      }
    >
      {q.isLoading && <span className="spin" />}
      {q.error && <Notice kind="bad">{errorMessage(q.error)}</Notice>}
      {imp && <ImpactBody imp={imp} />}
      <div className="row" style={{ marginTop: 12 }}>
        <span className="label">사유</span>
        <input className="input" style={{ flex: 1 }} placeholder="예: 금액 오류가 있는 판이라 되돌림" value={reason} onChange={(e) => setReason(e.target.value)} />
      </div>
    </Modal>
  )
}

export function ImpactBody({ imp }: { imp: Impact }) {
  const before = imp.before
  const after = imp.after
  const dColl = after.collectionTotal - before.collectionTotal
  const dExp = after.expenditureTotal - before.expenditureTotal
  const funds = new Map<string, { before: number; after: number }>()
  for (const l of before.byFund) funds.set(l.name, { before: l.total, after: 0 })
  for (const l of after.byFund) funds.set(l.name, { before: funds.get(l.name)?.before ?? 0, after: l.total })
  return (
    <>
      {imp.blocked.map((b) => (
        <Notice key={b} kind="bad">
          {b}
        </Notice>
      ))}
      <div className="row" style={{ marginTop: 8 }}>
        <div className="stat">
          <span>대상 파일</span>
          <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>
            {imp.file.fileName} (제{imp.file.versionNo}판)
          </strong>
        </div>
        <div className="stat">
          <span>귀속월</span>
          <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{imp.monthLabel}</strong>
        </div>
        {imp.current && (
          <div className="stat">
            <span>현재 적용 중 (취소될 판)</span>
            <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>
              {imp.current.fileName} (제{imp.current.versionNo}판)
            </strong>
          </div>
        )}
      </div>
      <div className="tableWrap" style={{ marginTop: 10 }}>
        <table className="table">
          <thead>
            <tr>
              <th />
              <th className="num">현재</th>
              <th className="num">실행 후</th>
              <th className="num">변동</th>
            </tr>
          </thead>
          <tbody>
            <tr>
              <td>거래 건수</td>
              <td className="num">{won(before.entryCount)}</td>
              <td className="num">{won(after.entryCount)}</td>
              <td className="num">{diff(after.entryCount - before.entryCount)}</td>
            </tr>
            <tr>
              <td>프로그램 수</td>
              <td className="num">{won(before.programCount)}</td>
              <td className="num">{won(after.programCount)}</td>
              <td className="num">{diff(after.programCount - before.programCount)}</td>
            </tr>
            <tr>
              <td>징수 합계</td>
              <td className="num">{won(before.collectionTotal)}</td>
              <td className="num">{won(after.collectionTotal)}</td>
              <td className={`num ${dColl > 0 ? 'diffcell--up' : dColl < 0 ? 'diffcell--down' : ''}`}>{diff(dColl)}</td>
            </tr>
            <tr>
              <td>지출 합계</td>
              <td className="num">{won(before.expenditureTotal)}</td>
              <td className="num">{won(after.expenditureTotal)}</td>
              <td className={`num ${dExp > 0 ? 'diffcell--up' : dExp < 0 ? 'diffcell--down' : ''}`}>{diff(dExp)}</td>
            </tr>
            {[...funds.entries()].map(([name, v]) => (
              <tr key={name}>
                <td className="muted">재원 · {name}</td>
                <td className="num">{won(v.before)}</td>
                <td className="num">{won(v.after)}</td>
                <td className="num">{diff(v.after - v.before)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {imp.dependents.length > 0 && (
        <div className="notice" style={{ marginTop: 10, display: 'block' }}>
          <strong>같은 달의 다른 파일 거래는 유지됩니다 (연결 재검증 대상)</strong>
          <ul className="list">
            {imp.dependents.map((d) => (
              <li key={d.fileId}>
                {d.fileName} · {d.dataKindLabel} · 같은 학생·프로그램 거래 {d.entryCount}건
              </li>
            ))}
          </ul>
        </div>
      )}
      {imp.warnings.map((w) => (
        <div key={w} className="notice notice--soon" style={{ marginTop: 8 }}>
          {w}
        </div>
      ))}
    </>
  )
}
