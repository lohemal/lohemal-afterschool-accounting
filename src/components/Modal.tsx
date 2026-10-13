import { useEffect, type ReactNode } from 'react'

/** 확인 창. Esc 로 닫힌다. 되돌리기 어려운 작업 앞에 쓴다. */
export function Modal({
  title,
  onClose,
  children,
  footer,
  width = 640,
}: {
  title: string
  onClose: () => void
  children: ReactNode
  footer?: ReactNode
  width?: number
}) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', h)
    return () => window.removeEventListener('keydown', h)
  }, [onClose])
  return (
    <div className="modal__backdrop" onMouseDown={onClose}>
      <div className="modal" style={{ width }} onMouseDown={(e) => e.stopPropagation()} role="dialog" aria-label={title}>
        <div className="modal__head">
          <strong>{title}</strong>
          <button type="button" className="modal__close" onClick={onClose} aria-label="닫기">
            ×
          </button>
        </div>
        <div className="modal__body">{children}</div>
        {footer && <div className="modal__foot">{footer}</div>}
      </div>
    </div>
  )
}
