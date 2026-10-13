import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'

import { Modal } from '@/components/Modal'
import { NoteBadge, StatusBadge } from '@/components/ReconTable'
import { Button, Notice, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { diff, shortDateTime, won } from '@/lib/format'
import type { ReconRow, TraceEntry } from '@/ipc/types'

const NOTE_OPTIONS: [string, string][] = [
  ['todo', '확인 필요'],
  ['investigating', '확인 중'],
  ['explained', '사유 확인 (차액은 남음)'],
]

/** 차액 상세 — 검증 요약 · 징수 내역 · 지급 내역 · 관련 학생·업체 · 원본 행 · 확인 메모. */
export function ReconDetailModal({ fiscalMonthId, row, onClose }: { fiscalMonthId: number; row: ReconRow; onClose: () => void }) {
  const qc = useQueryClient()
  const toast = useToast()
  const q = useQuery({
    queryKey: ['reconDetail', fiscalMonthId, row.unitKind, row.unitId, row.itemSet],
    queryFn: () => api.reconDetail(fiscalMonthId, row.unitKind, row.unitId, row.itemSet),
  })
  const [noteStatus, setNoteStatus] = useState<string>(row.noteStatus === 'none' ? 'todo' : row.noteStatus)
  const [reason, setReason] = useState('')
  const [source, setSource] = useState<number | null>(null)
  const [busy, setBusy] = useState(false)
  const d = q.data

  async function saveNote() {
    setBusy(true)
    try {
      await api.reconNoteSave(fiscalMonthId, row.unitKind, row.unitId, row.itemSet, noteStatus, reason)
      toast.show('확인 상태를 기록했습니다. 검증 결과 자체는 바뀌지 않습니다.')
      setReason('')
      await qc.invalidateQueries()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Modal title={`차액 상세 — ${row.unitName} · ${row.itemLabel}`} onClose={onClose} width={1180}>
      {q.isLoading && <span className="spin" />}
      {q.error && <Notice kind="bad">{errorMessage(q.error)}</Notice>}
      {d && (
        <>
          <div className="row">
            <div className="stat"><span>귀속월</span><strong style={{ fontFamily: 'inherit', fontSize: 13 }}>{d.monthLabel}</strong></div>
            <div className="stat"><span>최초 징수</span><strong>{won(d.row.initial)}</strong></div>
            <div className="stat"><span>추가징수</span><strong>{won(d.row.extra)}</strong></div>
            <div className="stat"><span>환불</span><strong>{won(d.row.refund)}</strong></div>
            <div className="stat"><span>징수 기준액</span><strong>{won(d.row.basis)}</strong></div>
            <div className="stat"><span>지급액</span><strong>{won(d.row.expense)}</strong></div>
            <div className={`stat ${d.row.status === 'mismatch' ? 'stat--bad' : ''}`}><span>차액 (기준액 − 지급액)</span><strong>{d.row.status === 'ok' || d.row.status === 'mismatch' ? diff(d.row.diff) : '—'}</strong></div>
            <div className="stat"><span>검증 · 확인</span><strong style={{ fontFamily: 'inherit' }}><StatusBadge row={d.row} /> <NoteBadge row={d.row} /></strong></div>
          </div>
          {d.members.length > 0 && <p className="muted" style={{ margin: '8px 0 0' }}>그룹 구성: {d.members.join(' + ')} — 그룹 전체로만 검증하며 개별 프로그램 지급액은 확인되지 않습니다.</p>}

          <h3 className="card__title" style={{ marginTop: 14 }}>재원별 대조</h3>
          <div className="tableWrap">
            <table className="table">
              <thead>
                <tr>
                  <th>재원</th>
                  <th className="num">징수 기준액</th>
                  <th className="num">지급액</th>
                  <th className="num">차액</th>
                  <th>판정</th>
                </tr>
              </thead>
              <tbody>
                {d.row.funds.map((f) => (
                  <tr key={f.fundName} className={f.state === 'mismatch' ? 'row--error' : ''}>
                    <td>{f.fundName}</td>
                    <td className="num">{f.basis === null ? <span className="muted">자료 없음</span> : won(f.basis)}</td>
                    <td className="num">{f.expense === null ? <span className="muted">자료 없음</span> : won(f.expense)}</td>
                    <td className="num">{f.diff === null ? '—' : diff(f.diff)}</td>
                    <td>{f.state === 'ok' ? '일치' : f.state === 'mismatch' ? '불일치' : f.state === 'no_basis' ? '대조 자료 없음 (지급만)' : '대조 자료 없음 (징수만)'}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          {d.row.hints.length > 0 && (
            <div className="notice" style={{ display: 'block', marginTop: 10 }}>
              <strong>확인 단서</strong> <span className="muted">— 원인을 확정하는 판단이 아니라 살펴볼 곳입니다.</span>
              <ul className="list">
                {d.row.hints.map((h) => (
                  <li key={h}>{h}</li>
                ))}
              </ul>
            </div>
          )}

          <h3 className="card__title" style={{ marginTop: 14 }}>징수 내역 (최초 징수 · 추가징수 · 환불)</h3>
          <TraceTable rows={d.collection} onSource={setSource} />
          <h3 className="card__title" style={{ marginTop: 14 }}>지급 내역</h3>
          <TraceTable rows={d.expenditure} onSource={setSource} />

          <h3 className="card__title" style={{ marginTop: 14 }}>관련 학생 · 업체 (유효 거래)</h3>
          <div className="tableWrap" style={{ maxHeight: 220 }}>
            <table className="table">
              <thead>
                <tr>
                  <th>학생 / 업체 / 강사</th>
                  <th className="num">거래</th>
                  <th className="num">귀속 금액</th>
                </tr>
              </thead>
              <tbody>
                {d.parties.map((p) => (
                  <tr key={p.name}>
                    <td>{p.name}</td>
                    <td className="num">{won(p.count)}</td>
                    <td className="num">{won(p.total)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <h3 className="card__title" style={{ marginTop: 14 }}>확인 상태 · 메모</h3>
          <div className="row">
            <select className="select" value={noteStatus} onChange={(e) => setNoteStatus(e.target.value)}>
              {NOTE_OPTIONS.map(([v, l]) => (
                <option key={v} value={v}>
                  {l}
                </option>
              ))}
            </select>
            <input className="input" style={{ flex: 1 }} placeholder="확인 사유 (예: 9월 전환 과정의 기준 자료 차이, 담당자 확인 완료)" value={reason} onChange={(e) => setReason(e.target.value)} />
            <Button variant="primary" disabled={busy} onClick={saveNote}>
              기록
            </Button>
          </div>
          <p className="muted" style={{ margin: '6px 0 0', fontSize: 12 }}>
            확인 상태는 시스템의 검증 결과를 바꾸지 않습니다. 이후 자료가 바뀌어 차액·구성이 달라지면 '재확인 필요'로 표시되고, 이전 메모는 그대로 남습니다.
          </p>
          {d.notes.length > 0 && (
            <div className="tableWrap" style={{ marginTop: 8 }}>
              <table className="table">
                <thead>
                  <tr>
                    <th>기록 시각</th>
                    <th>상태</th>
                    <th>사유</th>
                    <th>기준</th>
                  </tr>
                </thead>
                <tbody>
                  {d.notes.map((n) => (
                    <tr key={n.id}>
                      <td className="muted">{shortDateTime(n.createdAt)}</td>
                      <td>{NOTE_OPTIONS.find(([v]) => v === n.status)?.[1] ?? n.status}</td>
                      <td className="wrap">{n.reason}</td>
                      <td className="muted">{n.fingerprint === d.row.fingerprint ? '현재 자료 기준' : '이전 자료 기준'}</td>
                    </tr>
                  ))}
                </tbody>
              </table>
            </div>
          )}
        </>
      )}
      {source !== null && <SourceRowModal sourceRowId={source} onClose={() => setSource(null)} />}
    </Modal>
  )
}

function TraceTable({ rows, onSource }: { rows: TraceEntry[]; onSource: (id: number) => void }) {
  if (rows.length === 0) return <p className="muted">거래가 없습니다.</p>
  return (
    <div className="tableWrap" style={{ maxHeight: 300 }}>
      <table className="table">
        <thead>
          <tr>
            <th>유형</th>
            <th>프로그램</th>
            <th>학생 / 업체</th>
            <th>재원</th>
            <th>항목</th>
            <th className="num">금액</th>
            <th className="num">귀속</th>
            <th>귀속월 · 처리일</th>
            <th>원본 파일 · 판</th>
            <th>시트 · 행</th>
            <th className="center">유효</th>
          </tr>
        </thead>
        <tbody>
          {rows.map((t) => (
            <tr key={t.entryId} className={t.state !== 'active' ? 'row--off' : ''}>
              <td>{t.kindLabel}</td>
              <td>{t.programName}{t.viaAllocation && <span className="badge badge--navy" style={{ marginLeft: 4 }}>배분</span>}</td>
              <td>{t.party}</td>
              <td>
                {t.fundName}
                {t.fundText && t.fundText !== t.fundName && <span className="muted" style={{ marginLeft: 4, fontSize: 11 }}>({t.fundText.replace(/\s+/g, ' ')})</span>}
              </td>
              <td>{t.itemName}</td>
              <td className="num">{won(t.amount)}</td>
              <td className="num">{t.state === 'active' ? won(t.attributed) : '—'}</td>
              <td className="muted">{t.monthLabel}{t.processedOn ? ` · ${t.processedOn}` : ''}</td>
              <td>{t.fileName ? `${t.fileName} (제${t.versionNo}판)` : <span className="muted">직접 등록</span>}</td>
              <td>
                {t.sourceRowId !== null ? (
                  <a href="#" onClick={(e) => { e.preventDefault(); onSource(t.sourceRowId!) }}>
                    {t.sheet} {t.rowNo}행
                  </a>
                ) : (
                  <span className="muted">—</span>
                )}
              </td>
              <td className="center">{t.state === 'active' ? <span className="badge badge--ok">유효</span> : <span className="badge badge--muted">취소됨</span>}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

export function SourceRowModal({ sourceRowId, onClose }: { sourceRowId: number; onClose: () => void }) {
  const toast = useToast()
  const q = useQuery({ queryKey: ['sourceRow', sourceRowId], queryFn: () => api.sourceRowView(sourceRowId) })
  const s = q.data
  return (
    <Modal
      title="원본 행"
      onClose={onClose}
      width={760}
      footer={
        <>
          {s && (
            <Button onClick={() => api.openStoredFile(s.fileId).catch((e) => toast.show(errorMessage(e), true))}>
              보관된 원본 파일 열기
            </Button>
          )}
          <Button onClick={onClose}>닫기</Button>
        </>
      }
    >
      {q.error && <Notice kind="bad">{errorMessage(q.error)}</Notice>}
      {s && (
        <>
          <p style={{ marginTop: 0 }}>
            <strong>{s.fileName}</strong> (제{s.versionNo}판) · 시트 <strong>{s.sheet}</strong> · <strong>{s.rowNo}행</strong>
            <span className="muted"> · 업로드 당시 셀 값 그대로. 원본 파일은 바꾸지 않습니다.</span>
          </p>
          <div className="tableWrap">
            <table className="table">
              <tbody>
                {s.cells.map((c, i) => (
                  <tr key={i}>
                    <td className="muted" style={{ width: 60 }}>{String.fromCharCode(65 + (i % 26))}{i >= 26 ? Math.floor(i / 26) : ''}</td>
                    <td className="wrap">{c || <span className="muted">(빈 칸)</span>}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </>
      )}
    </Modal>
  )
}
