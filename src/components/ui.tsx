import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from 'react'

/* ── 페이지 머리 ─────────────────────────────────────── */

export function Page({
  title,
  sub,
  actions,
  children,
}: {
  title: string
  sub?: ReactNode
  actions?: ReactNode
  children: ReactNode
}) {
  return (
    <div>
      <div className="page__head">
        <div>
          <h1 className="page__title">{title}</h1>
          {sub && <p className="page__sub">{sub}</p>}
        </div>
        {actions && <div className="page__actions">{actions}</div>}
      </div>
      {children}
    </div>
  )
}

export function Card({ title, children, right }: { title?: ReactNode; right?: ReactNode; children: ReactNode }) {
  return (
    <section className="card">
      {title && (
        <h2 className="card__title">
          {title}
          {right && <span style={{ marginLeft: 'auto', fontWeight: 400 }}>{right}</span>}
        </h2>
      )}
      {children}
    </section>
  )
}

/** 아직 구현되지 않은 기능 — 무엇이 언제 오는지 적는다. 가짜 자료는 절대 보여주지 않는다. */
export function Soon({ phase, children }: { phase: string; children: ReactNode }) {
  return (
    <div className="notice notice--soon">
      <span className="badge badge--soon">준비 중 · {phase}</span>
      <div>{children}</div>
    </div>
  )
}

export function Notice({ kind = 'info', children }: { kind?: 'info' | 'bad' | 'ok'; children: ReactNode }) {
  const cls = kind === 'bad' ? 'notice notice--bad' : kind === 'ok' ? 'notice notice--ok' : 'notice'
  return <div className={cls}>{children}</div>
}

export function Empty({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <strong>{title}</strong>
      {children}
    </div>
  )
}

export function Button({
  variant = 'default',
  size,
  ...rest
}: React.ButtonHTMLAttributes<HTMLButtonElement> & { variant?: 'default' | 'primary'; size?: 'sm' }) {
  const cls = ['btn', variant === 'primary' ? 'btn--primary' : '', size === 'sm' ? 'btn--sm' : '']
    .filter(Boolean)
    .join(' ')
  return <button type="button" className={cls} {...rest} />
}

/* ── 토스트 ──────────────────────────────────────────── */

interface ToastCtx {
  show: (msg: string, bad?: boolean) => void
}

const TCtx = createContext<ToastCtx>({ show: () => {} })

export function ToastProvider({ children }: { children: ReactNode }) {
  const [item, setItem] = useState<{ msg: string; bad: boolean; id: number } | null>(null)
  const show = useCallback((msg: string, bad = false) => setItem({ msg, bad, id: Date.now() }), [])
  useEffect(() => {
    if (!item) return
    const t = setTimeout(() => setItem(null), item.bad ? 6000 : 3000)
    return () => clearTimeout(t)
  }, [item])
  return (
    <TCtx.Provider value={{ show }}>
      {children}
      {item && <div className={item.bad ? 'toast toast--bad' : 'toast'}>{item.msg}</div>}
    </TCtx.Provider>
  )
}

export function useToast() {
  return useContext(TCtx)
}
