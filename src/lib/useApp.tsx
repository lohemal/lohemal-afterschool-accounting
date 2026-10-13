/**
 * 앱 전역 상태 — 부트스트랩 결과(학년도 목록·현재 학년도·건수)와 선택 학년도.
 * 실제 자료는 전부 Rust 쪽 SQLite 에 있고, 여기서는 캐시만 든다.
 */

import { useQuery, useQueryClient } from '@tanstack/react-query'
import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from 'react'

import { api } from '@/ipc/api'
import type { Bootstrap, SchoolYear } from '@/ipc/types'

interface AppCtx {
  boot: Bootstrap
  /** 화면에서 보고 있는 학년도 (기본 = 현재 학년도) */
  year: SchoolYear | null
  setYearId: (id: number) => void
  refresh: () => Promise<void>
}

const Ctx = createContext<AppCtx | null>(null)

export function AppProvider({
  children,
  fallback,
}: {
  children: ReactNode
  fallback: (s: { loading: boolean; error: unknown; retry: () => void }) => ReactNode
}) {
  const qc = useQueryClient()
  const q = useQuery({ queryKey: ['bootstrap'], queryFn: api.bootstrap })
  const [pickedId, setPickedId] = useState<number | null>(null)

  const refresh = useCallback(async () => {
    await qc.invalidateQueries()
  }, [qc])

  const value = useMemo<AppCtx | null>(() => {
    if (!q.data) return null
    const years = q.data.years
    const year =
      years.find((y) => y.id === pickedId) ?? q.data.currentYear ?? years[0] ?? null
    return { boot: q.data, year, setYearId: setPickedId, refresh }
  }, [q.data, pickedId, refresh])

  if (!value) {
    return <>{fallback({ loading: q.isLoading, error: q.error, retry: () => void q.refetch() })}</>
  }
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>
}

export function useApp(): AppCtx {
  const v = useContext(Ctx)
  if (!v) throw new Error('AppProvider 바깥에서 useApp 을 불렀습니다.')
  return v
}
