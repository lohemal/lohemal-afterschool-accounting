import { useQuery, useQueryClient } from '@tanstack/react-query'
import { useState } from 'react'
import { Link } from 'react-router-dom'

import { Button, Card, Empty, Notice, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import { won } from '@/lib/format'

/** 설정 → 프로그램 · 별칭 · 그룹 관리. */
export function ProgramAdmin({ yearId }: { yearId: number }) {
  const toast = useToast()
  const qc = useQueryClient()
  const programs = useQuery({ queryKey: ['programs', yearId], queryFn: () => api.programList(yearId) })
  const groups = useQuery({ queryKey: ['groups', yearId], queryFn: () => api.programGroupList(yearId) })
  const [newName, setNewName] = useState('')
  const [aliasFor, setAliasFor] = useState<number | ''>('')
  const [aliasText, setAliasText] = useState('')
  const [groupName, setGroupName] = useState('')
  const [members, setMembers] = useState<number[]>([])
  const [busy, setBusy] = useState(false)

  async function run(label: string, f: () => Promise<unknown>) {
    setBusy(true)
    try {
      await f()
      await qc.invalidateQueries({ queryKey: ['programs'] })
      await qc.invalidateQueries({ queryKey: ['groups'] })
      toast.show(label)
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  return (
    <Card title="프로그램 · 표기 별칭 · 그룹">
      <p className="muted" style={{ margin: '0 0 10px' }}>
        파일마다 다른 표기('코딩A' / '코딩')를 대표 프로그램에 잇습니다. 비슷한 이름을 자동으로 합치지 않으며, 여기서 확정한 규칙만 업로드에 쓰입니다. 두 프로그램이 한 지출 행에 합산된 표기('우쿨렐레&통기타')는 그룹으로 둡니다.
        세부반('로봇과학A')과 통합 프로그램('로봇과학')의 연결은 <Link to="/programs">프로그램 매핑</Link>에서 관리합니다.
      </p>
      {programs.error && <Notice kind="bad">{errorMessage(programs.error)}</Notice>}
      {programs.data && programs.data.length === 0 && <Empty title="등록된 프로그램이 없습니다.">첫 업로드의 미리보기에서 표기를 확정하면 여기에 쌓입니다.</Empty>}
      {programs.data && programs.data.length > 0 && (
        <div className="tableWrap">
          <table className="table">
            <thead>
              <tr>
                <th>프로그램</th>
                <th>통합 · 세부반</th>
                <th>파일 표기(별칭)</th>
                <th className="num">유효 거래</th>
              </tr>
            </thead>
            <tbody>
              {programs.data.map((p) => (
                <tr key={p.id}>
                  <td>{p.name}</td>
                  <td>
                    {p.childCount > 0 && <span className="badge badge--navy">통합 · 세부반 {p.childCount}개</span>}
                    {p.parentName && <span className="muted">→ {p.parentName} 의 세부반</span>}
                    {p.childCount === 0 && !p.parentName && <span className="muted">—</span>}
                  </td>
                  <td>
                    <div className="chips">
                      {p.aliases.map((a) => (
                        <span className="chip" key={a}>
                          {a}
                          <button type="button" title="별칭 삭제" disabled={busy} onClick={() => run(`별칭 '${a}' 을(를) 지웠습니다.`, () => api.programAliasDelete(yearId, a))}>
                            ×
                          </button>
                        </span>
                      ))}
                      {p.aliases.length === 0 && <span className="muted">—</span>}
                    </div>
                  </td>
                  <td className="num">{won(p.entryCount)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="row" style={{ marginTop: 10 }}>
        <input className="input input--sm" placeholder="새 프로그램 이름" value={newName} onChange={(e) => setNewName(e.target.value)} style={{ width: 200 }} />
        <Button size="sm" disabled={busy || !newName.trim()} onClick={() => run('프로그램을 등록했습니다.', async () => { await api.programMapSave({ schoolYearId: yearId, alias: newName.trim(), target: 'new_program' }); setNewName('') })}>
          프로그램 추가
        </Button>
        <span style={{ width: 16 }} />
        <select className="select input--sm" value={aliasFor} onChange={(e) => setAliasFor(e.target.value === '' ? '' : Number(e.target.value))}>
          <option value="">별칭을 붙일 프로그램</option>
          {(programs.data ?? []).map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <input className="input input--sm" placeholder="파일 표기 (예: 코딩A)" value={aliasText} onChange={(e) => setAliasText(e.target.value)} style={{ width: 180 }} />
        <Button size="sm" disabled={busy || !aliasFor || !aliasText.trim()} onClick={() => run('별칭을 저장했습니다.', async () => { await api.programMapSave({ schoolYearId: yearId, alias: aliasText.trim(), target: 'program', programId: Number(aliasFor) }); setAliasText('') })}>
          별칭 추가
        </Button>
      </div>

      <h3 className="card__title" style={{ marginTop: 16 }}>프로그램 그룹</h3>
      {groups.data && groups.data.length > 0 && (
        <div className="tableWrap" style={{ marginBottom: 8 }}>
          <table className="table">
            <thead>
              <tr>
                <th>그룹</th>
                <th>구성 프로그램</th>
              </tr>
            </thead>
            <tbody>
              {groups.data.map((g) => (
                <tr key={g.id}>
                  <td>{g.name}</td>
                  <td>{g.memberNames.join(' + ')}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      <div className="row">
        <input className="input input--sm" placeholder="그룹 이름 (예: 우쿨렐레&통기타)" value={groupName} onChange={(e) => setGroupName(e.target.value)} style={{ width: 220 }} />
        <select
          className="select input--sm"
          multiple
          value={members.map(String)}
          onChange={(e) => setMembers([...e.target.selectedOptions].map((o) => Number(o.value)))}
          style={{ minWidth: 220, height: 80 }}
        >
          {(programs.data ?? []).map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <Button size="sm" disabled={busy || !groupName.trim() || members.length < 2} onClick={() => run('그룹을 만들었습니다.', async () => { await api.programMapSave({ schoolYearId: yearId, alias: groupName.trim(), target: 'new_group', memberIds: members }); setGroupName(''); setMembers([]) })}>
          그룹 만들기 (Ctrl 로 여러 개 선택)
        </Button>
      </div>
    </Card>
  )
}

/** 설정 → 재원 별칭. */
export function FundAliasAdmin() {
  const toast = useToast()
  const qc = useQueryClient()
  const aliases = useQuery({ queryKey: ['fundAliases'], queryFn: api.fundAliasList })
  const cat = useQuery({ queryKey: ['catalog'], queryFn: api.catalogList })
  const [fund, setFund] = useState<number | ''>('')
  const [text, setText] = useState('')
  const [busy, setBusy] = useState(false)

  async function run(label: string, f: () => Promise<unknown>) {
    setBusy(true)
    try {
      await f()
      await qc.invalidateQueries({ queryKey: ['fundAliases'] })
      toast.show(label)
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  const grouped = new Map<string, { id: number; alias: string; isBuiltin: boolean }[]>()
  for (const a of aliases.data ?? []) {
    const list = grouped.get(a.fundTypeName) ?? []
    list.push({ id: a.id, alias: a.alias, isBuiltin: a.isBuiltin })
    grouped.set(a.fundTypeName, list)
  }

  return (
    <Card title="재원 별칭 (엑셀 표기 → 재원)">
      <p className="muted" style={{ margin: '0 0 10px' }}>
        '초3 지원금' · '3학년 지원금' 은 방과후이용권과 같은 재원입니다. '초3 수익자 부담금(초과금)' 은 다른 재원이라 합치지 않습니다. 별칭은 공백을 뗀 글자로 비교합니다.
      </p>
      <div className="tableWrap">
        <table className="table">
          <thead>
            <tr>
              <th>재원</th>
              <th>표기</th>
            </tr>
          </thead>
          <tbody>
            {[...grouped.entries()].map(([name, list]) => (
              <tr key={name}>
                <td>{name}</td>
                <td>
                  <div className="chips">
                    {list.map((a) => (
                      <span className="chip" key={a.id}>
                        {a.alias}
                        {!a.isBuiltin && (
                          <button type="button" title="삭제" disabled={busy} onClick={() => run('별칭을 지웠습니다.', () => api.fundAliasDelete(a.id))}>
                            ×
                          </button>
                        )}
                      </span>
                    ))}
                  </div>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div className="row" style={{ marginTop: 10 }}>
        <select className="select input--sm" value={fund} onChange={(e) => setFund(e.target.value === '' ? '' : Number(e.target.value))}>
          <option value="">재원 선택</option>
          {(cat.data?.fundTypes ?? []).filter((f) => f.isActive).map((f) => (
            <option key={f.id} value={f.id}>
              {f.name}
            </option>
          ))}
        </select>
        <input className="input input--sm" placeholder="엑셀 표기 (예: 3학년 바우처)" value={text} onChange={(e) => setText(e.target.value)} style={{ width: 220 }} />
        <Button size="sm" disabled={busy || !fund || !text.trim()} onClick={() => run('재원 별칭을 저장했습니다.', async () => { await api.fundAliasSave(Number(fund), text.trim()); setText('') })}>
          별칭 추가
        </Button>
      </div>
    </Card>
  )
}
