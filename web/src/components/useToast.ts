import { createContext, useContext } from 'react'

export type ToastKind = 'success' | 'error' | 'info'

export interface ToastItem {
  id: number
  kind: ToastKind
  title: string
  detail?: string
  action?: { label: string; onClick: () => void }
}

export interface ToastApi {
  push: (t: Omit<ToastItem, 'id'>) => void
  success: (title: string, detail?: string, action?: ToastItem['action']) => void
  error: (title: string, detail?: string, action?: ToastItem['action']) => void
  info: (title: string, detail?: string, action?: ToastItem['action']) => void
}

/** A toast's action (each one in the app is an Undo) also runs from the
 *  keyboard with Ctrl+Z (⌘Z on a Mac), wherever the focus is, outside a
 *  text field: the toast sits at the end of the page, many Tabs away. */
const MAC = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent)
export const ACTION_KEYS = MAC ? 'Meta+Z' : 'Control+Z'
export const ACTION_KEYS_SAID = MAC ? 'Command+Z' : 'Control+Z'

export const ToastContext = createContext<ToastApi | null>(null)

export function useToast(): ToastApi {
  const ctx = useContext(ToastContext)
  if (!ctx) throw new Error('useToast must be used inside ToastProvider')
  return ctx
}
