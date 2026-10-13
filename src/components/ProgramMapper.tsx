import { useQuery } from '@tanstack/react-query'
import { useState } from 'react'

import { Button, useToast } from '@/components/ui'
import { api, errorMessage } from '@/ipc/api'
import type { ProgramMapInput, UnresolvedProgram } from '@/ipc/types'

/**
 * 등록되지 않은 프로그램 표기를 기존 프로그램·그룹에 잇거나 새로 만든다.
 * 비슷한 이름은 '제안'으로만 보여 주고, 확정은 사용자가 한다.
 */
export function ProgramMapper({
  schoolYearId,
  items,
  onSaved,
}: {
  schoolYearId: number
  items: UnresolvedProgram[]
  onSaved: () => void
}) {
  const toast = useToast()
  const programs = useQuery({ queryKey: ['programs', schoolYearId], queryFn: () => api.programList(schoolYearId) })
  const groups = useQuery({ queryKey: ['groups', schoolYearId], queryFn: () => api.programGroupList(schoolYearId) })
  const [choice, setChoice] = useState<Record<string, string>>({})
  const [busy, setBusy] = useState(false)

  async function save(text: string) {
    const c = choice[text] ?? defaultChoice(items.find((i) => i.text === text)!)
    const input = toInput(schoolYearId, text, c)
    if (!input) return
    setBusy(true)
    try {
      await api.programMapSave(input)
      onSaved()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  async function saveAllNew() {
    setBusy(true)
    try {
      for (const it of items) {
        await api.programMapSave({ schoolYearId, alias: it.text, target: 'new_program' })
      }
      toast.show(`${items.length}개 표기를 새 프로그램으로 등록했습니다.`)
      onSaved()
    } catch (e) {
      toast.show(errorMessage(e), true)
    } finally {
      setBusy(false)
    }
  }

  if (items.length === 0) return null
  return (
    <div className="notice notice--soon" style={{ display: 'block' }}>
      <div className="row" style={{ marginBottom: 8 }}>
        <strong>등록되지 않은 프로그램 표기 {items.length}개</strong>
        <span className="muted">— 기존 프로그램에 잇거나 새로 등록해야 반영할 수 있습니다. 확정한 규칙은 다음 업로드에 다시 쓰입니다.</span>
        <Button size="sm" style={{ marginLeft: 'auto' }} disabled={busy} onClick={saveAllNew}>
          모두 새 프로그램으로 등록
        </Button>
      </div>
      <div className="tableWrap">
        <table className="table">
          <thead>
            <tr>
              <th>파일 표기</th>
              <th className="num">행</th>
              <th>연결</th>
              <th />
            </tr>
          </thead>
          <tbody>
            {items.map((it) => {
              const val = choice[it.text] ?? defaultChoice(it)
              return (
                <tr key={it.text}>
                  <td>
                    <strong>{it.text}</strong>
                  </td>
                  <td className="num">{it.count}</td>
                  <td>
                    <select className="select input--sm" value={val} onChange={(e) => setChoice({ ...choice, [it.text]: e.target.value })} style={{ minWidth: 260 }}>
                      {it.suggestionProgramId !== null && (
                        <option value={`p:${it.suggestionProgramId}`}>제안: '{it.suggestionName}' 에 연결 (반 접미만 다름)</option>
                      )}
                      <option value="new">'{it.text}' 이름으로 새 프로그램 등록</option>
                      <optgroup label="기존 프로그램에 연결">
                        {(programs.data ?? []).map((p) => (
                          <option key={p.id} value={`p:${p.id}`}>
                            {p.name}
                          </option>
                        ))}
                      </optgroup>
                      {(groups.data ?? []).length > 0 && (
                        <optgroup label="프로그램 그룹에 연결">
                          {(groups.data ?? []).map((g) => (
                            <option key={g.id} value={`g:${g.id}`}>
                              {g.name} ({g.memberNames.join('+')})
                            </option>
                          ))}
                        </optgroup>
                      )}
                    </select>
                  </td>
                  <td>
                    <Button size="sm" variant="primary" disabled={busy} onClick={() => save(it.text)}>
                      확정
                    </Button>
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>
      <p className="muted" style={{ margin: '8px 0 0', fontSize: 12 }}>
        '우쿨렐레&통기타' 처럼 두 프로그램이 합산된 표기는 설정 → 프로그램에서 그룹을 먼저 만든 뒤 그룹에 연결하세요. 그룹 금액은 개별 프로그램에 나누지 않습니다.
      </p>
    </div>
  )
}

function defaultChoice(it: UnresolvedProgram): string {
  return it.suggestionProgramId !== null ? `p:${it.suggestionProgramId}` : 'new'
}

function toInput(schoolYearId: number, alias: string, choice: string): ProgramMapInput | null {
  if (choice === 'new') return { schoolYearId, alias, target: 'new_program' }
  if (choice.startsWith('p:')) return { schoolYearId, alias, target: 'program', programId: Number(choice.slice(2)) }
  if (choice.startsWith('g:')) return { schoolYearId, alias, target: 'group', groupId: Number(choice.slice(2)) }
  return null
}
