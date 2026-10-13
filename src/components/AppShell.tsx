import { NavLink, Outlet } from 'react-router-dom'

import { useApp } from '@/lib/useApp'

interface Item {
  to: string
  label: string
  soon?: boolean
}

const GROUPS: { label: string; items: Item[] }[] = [
  {
    label: '현황',
    items: [
      { to: '/', label: '대시보드' },
      { to: '/monthly', label: '월별 정산' },
    ],
  },
  {
    label: '자료',
    items: [
      { to: '/upload', label: '자료 업로드' },
      { to: '/funds', label: '재원별 관리', soon: true },
    ],
  },
  {
    label: '검증',
    items: [
      { to: '/diff', label: '차액 분석' },
      { to: '/history', label: '변경 이력' },
    ],
  },
  {
    label: '관리',
    items: [{ to: '/settings', label: '설정' }],
  },
]

export function AppShell() {
  const { boot, year, setYearId } = useApp()
  return (
    <div className="shell">
      <aside className="sidebar">
        <div className="sidebar__brand">
          방과후 회계 검증 매니저
          <small>{boot.schoolName || '학교명 미설정'}</small>
        </div>
        {GROUPS.map((g) => (
          <div className="sidebar__group" key={g.label}>
            <div className="sidebar__groupLabel">{g.label}</div>
            {g.items.map((it) => (
              <NavLink
                key={it.to}
                to={it.to}
                end={it.to === '/'}
                className={({ isActive }) => (isActive ? 'sidebar__link sidebar__link--on' : 'sidebar__link')}
              >
                {it.label}
                {it.soon && <span className="sidebar__soon">준비 중</span>}
              </NavLink>
            ))}
          </div>
        ))}
        <div className="sidebar__foot">
          v{boot.app.version} · 스키마 v{boot.app.schemaVersion}
          <br />
          자료는 이 컴퓨터에만 저장됩니다.
        </div>
      </aside>
      <div className="main">
        <header className="topbar">
          <span className="topbar__title">{year ? year.name : '학년도 없음'}</span>
          <span className="muted" style={{ fontSize: 12 }}>
            {year ? `${year.startYm} ~ ${year.endYm}` : '설정에서 학년도를 먼저 만들어 주세요.'}
          </span>
          <span className="topbar__spacer" />
          {boot.years.length > 0 && (
            <label className="topbar__year">
              보는 학년도
              <select
                className="select input--sm"
                style={{ height: 24 }}
                value={year?.id ?? ''}
                onChange={(e) => setYearId(Number(e.target.value))}
              >
                {boot.years.map((y) => (
                  <option key={y.id} value={y.id}>
                    {y.name}
                    {y.isCurrent ? ' (현재)' : ''}
                  </option>
                ))}
              </select>
            </label>
          )}
        </header>
        <main className="content">
          <Outlet />
        </main>
      </div>
    </div>
  )
}
