import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { FundAliasAdmin, ProgramAdmin } from '@/components/ProgramAdmin'
import { Button, Card, Notice, Page, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { shortDateTime } from '@/lib/format'
import { schoolYearOf } from '@/lib/months'
import { useApp } from '@/lib/useApp'
import type { AmountItem, CatalogInput, FundType } from '@/ipc/types'

export function SettingsPage() {
  const { boot, refresh, year } = useApp()
  const toast = useToast()
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })

  const [schoolName, setSchoolName] = useState(boot.schoolName)
  const [newYear, setNewYear] = useState(schoolYearOf(new Date()) + (boot.years.length ? 1 : 0))
  const [busy, setBusy] = useState(false)

  async function run(label: string, f: () => Promise<unknown>) {
    setBusy(true)
    try {
      await f()
      await refresh()
      toast.show(label)
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Page title="설정" sub="학교 정보, 학년도, 재원·금액 항목, 자료 위치를 관리합니다.">
      <Card title="학교">
        <div className="row">
          <span className="label">학교명</span>
          <input className="input" value={schoolName} onChange={(e) => setSchoolName(e.target.value)} style={{ width: 260 }} />
          <Button disabled={busy} onClick={() => run('학교명을 저장했습니다.', () => api.settingSet('school_name', schoolName))}>
            저장
          </Button>
        </div>
      </Card>

      <Card title="학년도">
        <p className="muted" style={{ margin: '0 0 10px' }}>
          학년도는 3월부터 다음 해 2월까지 12개 회계월로 만들어집니다. 현재 학년도는 하나뿐이며, 화면 위쪽에서 보는 학년도를 따로 고를 수
          있습니다.
        </p>
        {boot.years.length > 0 && (
          <div className="tableWrap" style={{ marginBottom: 12 }}>
            <table className="table">
              <thead>
                <tr>
                  <th>학년도</th>
                  <th>기간</th>
                  <th className="center">현재</th>
                  <th>만든 때</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {boot.years.map((y) => (
                  <tr key={y.id}>
                    <td>{y.name}</td>
                    <td className="muted">
                      {y.startYm} ~ {y.endYm}
                    </td>
                    <td className="center">{y.isCurrent ? <span className="badge badge--ok">현재</span> : ''}</td>
                    <td className="muted">{shortDateTime(y.createdAt)}</td>
                    <td>
                      {!y.isCurrent && (
                        <Button size="sm" disabled={busy} onClick={() => run(`${y.name}를 현재 학년도로 바꿨습니다.`, () => api.yearSetCurrent(y.id))}>
                          현재로
                        </Button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
        <div className="row">
          <span className="label">새 학년도</span>
          <input className="input" type="number" min={2000} max={2100} value={newYear} onChange={(e) => setNewYear(Number(e.target.value))} style={{ width: 110 }} />
          <Button variant="primary" disabled={busy} onClick={() => run(`${newYear}학년도를 만들었습니다.`, () => api.yearCreate(newYear))}>
            만들기
          </Button>
        </div>
      </Card>

      <div className="grid grid--2">
        <Card title="재원 유형">
          {cat.error && <Notice kind="bad">{errorMessage(cat.error)}</Notice>}
          {cat.data && (
            <CatalogEditor
              rows={cat.data.fundTypes}
              parents={cat.data.fundTypes}
              busy={busy}
              onSave={(input) => run('재원을 저장했습니다.', () => api.fundTypeSave(input))}
            />
          )}
        </Card>
        <Card title="금액 항목">
          {cat.data && (
            <CatalogEditor
              rows={cat.data.amountItems}
              busy={busy}
              onSave={(input) => run('금액 항목을 저장했습니다.', () => api.amountItemSave(input))}
            />
          )}
        </Card>
      </div>

      {year && <ProgramAdmin yearId={year.id} />}
      <FundAliasAdmin />

      <Card title="자료 위치">
        <div className="row">
          <span className="label">DB 파일</span>
          <span className="mono">{boot.app.dbPath}</span>
        </div>
        <div className="row">
          <span className="label">스키마</span>
          <span>v{boot.app.schemaVersion}</span>
          <span className="label" style={{ marginLeft: 16 }}>
            프로그램
          </span>
          <span>v{boot.app.version}</span>
          <Button size="sm" style={{ marginLeft: 'auto' }} onClick={() => api.openDataFolder().catch((e) => toast.show(errorMessage(e), true))}>
            자료 폴더 열기
          </Button>
        </div>
        <p className="muted" style={{ margin: '10px 0 0' }}>
          모든 자료는 이 컴퓨터의 SQLite 파일에만 저장되며 외부로 전송되지 않습니다. 올린 엑셀 원본은 자료 폴더의 uploads/ 에 보관됩니다. DB 백업·복원은 다음 단계에 붙습니다.
        </p>
      </Card>
    </Page>
  )
}

/* ── 재원·항목 편집 표 (이름 바꾸기, 사용자 항목 추가·끄기) ───── */

function CatalogEditor({
  rows,
  parents,
  busy,
  onSave,
}: {
  rows: (FundType | AmountItem)[]
  parents?: FundType[]
  busy: boolean
  onSave: (input: CatalogInput) => void
}) {
  const [drafts, setDrafts] = useState<Record<number, string>>({})
  const [newName, setNewName] = useState('')

  return (
    <>
      <div className="tableWrap">
        <table className="table">
          <thead>
            <tr>
              <th>이름</th>
              {parents && <th>상위</th>}
              <th className="center">구분</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {rows.map((r) => {
              const draft = drafts[r.id] ?? r.name
              const dirty = draft !== r.name
              const parentId = 'parentId' in r ? r.parentId : null
              return (
                <tr key={r.id}>
                  <td>
                    <input
                      className="input input--sm"
                      value={draft}
                      onChange={(e) => setDrafts({ ...drafts, [r.id]: e.target.value })}
                      style={{ width: '100%', opacity: r.isActive ? 1 : 0.5 }}
                    />
                  </td>
                  {parents && <td className="muted">{parents.find((p) => p.id === parentId)?.name ?? '—'}</td>}
                  <td className="center">
                    <span className={r.isBuiltin ? 'badge badge--navy' : 'badge badge--muted'}>{r.isBuiltin ? '기본' : '사용자'}</span>
                    {'mergedIntoId' in r && r.mergedIntoId !== null && <span className="badge badge--muted" style={{ marginLeft: 4 }}>통합됨</span>}
                  </td>
                  <td>
                    <div className="row" style={{ gap: 4 }}>
                      <Button size="sm" disabled={busy || !dirty} onClick={() => onSave({ id: r.id, name: draft, parentId })}>
                        저장
                      </Button>
                      {!r.isBuiltin && (
                        <Button size="sm" disabled={busy} onClick={() => onSave({ id: r.id, name: r.name, parentId, isActive: !r.isActive })}>
                          {r.isActive ? '끄기' : '켜기'}
                        </Button>
                      )}
                    </div>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      <div className="row" style={{ marginTop: 10 }}>
        <input className="input input--sm" placeholder="새 항목 이름" value={newName} onChange={(e) => setNewName(e.target.value)} style={{ width: 200 }} />
        <Button
          size="sm"
          disabled={busy || !newName.trim()}
          onClick={() => {
            onSave({ name: newName })
            setNewName('')
          }}
        >
          추가
        </Button>
      </div>
    </>
  )
}
