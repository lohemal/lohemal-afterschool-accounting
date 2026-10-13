import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { Modal } from '@/components/Modal'
import { Notice } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { shortDateTime, won } from '@/lib/format'
import { RowsTable } from '@/components/RowsTable'

const APPLY_LABEL: Record<string, string> = { apply: '반영', restore: '복원', cancel: '취소', replace: '교체됨' }

export function UploadDetailModal({ fileId, onClose }: { fileId: number; onClose: () => void }) {
  const q = useQuery({ queryKey: ['uploadDetail', fileId], queryFn: () => api.uploadDetail(fileId) })
  const [tab, setTab] = useState<'rows' | 'source' | 'history'>('rows')
  const d = q.data
  return (
    <Modal title="업로드 상세" onClose={onClose} width={1100}>
      {q.isLoading && <span className="spin" />}
      {q.error && <Notice kind="bad">{errorMessage(q.error)}</Notice>}
      {d && (
        <>
          <div className="row">
            <div className="stat">
              <span>파일</span>
              <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{d.file.fileName}</strong>
            </div>
            <div className="stat">
              <span>양식 · 자료 유형</span>
              <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>
                {d.file.formatLabel} · {d.file.dataKindLabel}
              </strong>
            </div>
            <div className="stat">
              <span>귀속월 · 판 · 상태</span>
              <strong style={{ fontFamily: 'inherit', fontSize: 13 }}>
                {d.file.fiscalMonthLabel} · 제{d.file.versionNo}판 · {d.file.statusLabel}
              </strong>
            </div>
            <div className="stat">
              <span>유효 거래 · 징수 · 지출</span>
              <strong>
                {won(d.file.entryCount)}건 · {won(d.file.activeCollectionTotal)} · {won(d.file.activeExpenditureTotal)}
              </strong>
            </div>
            <div className="stat">
              <span>보관 파일</span>
              <strong className="mono" style={{ fontSize: 11 }}>
                uploads/{d.file.storedName}
              </strong>
            </div>
          </div>
          {d.versions.length > 1 && (
            <div className="tableWrap" style={{ marginTop: 10 }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>판</th>
                    <th>파일</th>
                    <th className="center">상태</th>
                    <th className="num">유효 거래</th>
                    <th className="num">징수</th>
                    <th className="num">지출</th>
                    <th>업로드</th>
                  </tr>
                </thead>
                <tbody>
                  {d.versions.map((v) => (
                    <tr key={v.id} className={v.id === d.file.id ? 'row--check' : ''}>
                      <td>제{v.versionNo}판</td>
                      <td>{v.fileName}</td>
                      <td className="center">{v.statusLabel}</td>
                      <td className="num">{won(v.entryCount)}</td>
                      <td className="num">{won(v.activeCollectionTotal)}</td>
                      <td className="num">{won(v.activeExpenditureTotal)}</td>
                      <td className="muted">{shortDateTime(v.uploadedAt)}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          <div className="tabs" style={{ marginTop: 12 }}>
            <button type="button" className={tab === 'rows' ? 'tab tab--on' : 'tab'} onClick={() => setTab('rows')}>
              정규화 행 ({d.rows.length})
            </button>
            <button type="button" className={tab === 'source' ? 'tab tab--on' : 'tab'} onClick={() => setTab('source')}>
              원본 행 ({d.sourceRows.length})
            </button>
            <button type="button" className={tab === 'history' ? 'tab tab--on' : 'tab'} onClick={() => setTab('history')}>
              변경 이력 ({d.applies.length})
            </button>
          </div>
          {tab === 'rows' && <RowsTable rows={d.rows} />}
          {tab === 'source' && (
            <div className="tableWrap" style={{ maxHeight: 420 }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>시트</th>
                    <th className="num">행</th>
                    <th>셀 (원문 그대로)</th>
                  </tr>
                </thead>
                <tbody>
                  {d.sourceRows.slice(0, 500).map((s, i) => (
                    <tr key={i}>
                      <td>{s.sheet}</td>
                      <td className="num">{s.rowNo}</td>
                      <td className="wrap mono" style={{ fontSize: 11 }}>
                        {s.cells.filter((c) => c !== '').join(' | ')}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
          {tab === 'history' && (
            <div className="tableWrap">
              <table className="table">
                <thead>
                  <tr>
                    <th>시각</th>
                    <th>작업</th>
                    <th className="num">거래</th>
                    <th className="num">징수</th>
                    <th className="num">지출</th>
                    <th>메모</th>
                  </tr>
                </thead>
                <tbody>
                  {d.applies.map((a) => (
                    <tr key={a.id}>
                      <td className="muted">{shortDateTime(a.createdAt)}</td>
                      <td>{APPLY_LABEL[a.action] ?? a.action}</td>
                      <td className="num">{won(a.entryCount)}</td>
                      <td className="num">{won(a.collectionTotal)}</td>
                      <td className="num">{won(a.expenditureTotal)}</td>
                      <td className="wrap">{a.note}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
    </Modal>
  )
}
