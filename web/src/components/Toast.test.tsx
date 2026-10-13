import { useEffect } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, fireEvent, render, screen } from '@testing-library/react'
import { ToastProvider } from './Toast'
import { useToast, type ToastItem } from './useToast'

function Push({ t }: { t: Omit<ToastItem, 'id'> }) {
  const toast = useToast()
  useEffect(() => toast.push(t), [toast, t])
  return null
}

const undo = { kind: 'success', title: 'Hollow Knight won’t come up again', action: { label: 'Undo', onClick: () => {} } } as const

function show(t: Omit<ToastItem, 'id'>) {
  render(
    <ToastProvider>
      <Push t={t} />
    </ToastProvider>,
  )
}

const wait = (ms: number) => act(() => vi.advanceTimersByTime(ms))

describe('toasts (B-WEB-8)', () => {
  beforeEach(() => vi.useFakeTimers())
  afterEach(() => vi.useRealTimers())

  it('a plain one closes after 4.5 s', () => {
    show({ kind: 'success', title: 'Saved' })
    wait(4400)
    expect(screen.queryByText('Saved')).not.toBeNull()
    wait(200)
    expect(screen.queryByText('Saved')).toBeNull()
  })

  it('one with an Undo stays 8 s', () => {
    show(undo)
    wait(7900)
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeNull()
    wait(200)
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()
  })

  it('stays while the focus is on it, and closes a while after the focus leaves', () => {
    show(undo)
    wait(7000)
    act(() => screen.getByRole('button', { name: 'Undo' }).focus())
    wait(30_000)
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeNull()
    act(() => screen.getByRole('button', { name: 'Undo' }).blur())
    wait(1900)
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeNull()
    wait(200)
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()
  })

  it('stays while the pointer is on it, then goes on from where it stopped', () => {
    show(undo)
    wait(2000)
    const card = screen.getByRole('button', { name: 'Undo' }).closest('div.pointer-events-auto')!
    fireEvent.pointerEnter(card)
    wait(30_000)
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeNull()
    fireEvent.pointerLeave(card)
    wait(5900)
    expect(screen.queryByRole('button', { name: 'Undo' })).not.toBeNull()
    wait(200)
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()
  })

  it('says its action and the keys for it in the live region', () => {
    show({ ...undo, detail: 'Hollow Knight' })
    const live = screen.getByRole('status')
    expect(live.textContent).toContain('Hollow Knight won’t come up again')
    expect(live.textContent).toContain('Undo: press Control+Z.')
    expect(screen.getByRole('button', { name: 'Undo' }).getAttribute('aria-keyshortcuts')).toBe('Control+Z')
    // The message itself is in the page once, so finding it by its words finds one.
    expect(screen.getAllByText(/Hollow Knight won’t come up again/)).toHaveLength(1)
  })

  it('runs the newest action with Ctrl+Z, but not from a text field', () => {
    const onClick = vi.fn()
    render(
      <ToastProvider>
        <input aria-label="Search" />
        <Push t={{ ...undo, action: { label: 'Undo', onClick } }} />
      </ToastProvider>,
    )
    fireEvent.keyDown(screen.getByLabelText('Search'), { key: 'z', ctrlKey: true })
    expect(onClick).not.toHaveBeenCalled()
    fireEvent.keyDown(document.body, { key: 'z', ctrlKey: true })
    expect(onClick).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('button', { name: 'Undo' })).toBeNull()
    fireEvent.keyDown(document.body, { key: 'z', ctrlKey: true })
    expect(onClick).toHaveBeenCalledTimes(1)
  })

  it('gives the focus back to where it was when Undo or Dismiss closes it', () => {
    const shown = { ...undo, action: { label: 'Undo', onClick: () => {} } }
    function Page() {
      const toast = useToast()
      return (
        <button type="button" onClick={() => toast.push(shown)}>
          Not for me
        </button>
      )
    }
    render(
      <ToastProvider>
        <Page />
      </ToastProvider>,
    )
    const from = screen.getByRole('button', { name: 'Not for me' })
    for (const name of ['Undo', 'Dismiss']) {
      act(() => from.focus())
      fireEvent.click(from)
      act(() => screen.getByRole('button', { name }).focus())
      fireEvent.click(screen.getByRole('button', { name }))
      expect(screen.queryByRole('button', { name })).toBeNull()
      expect(document.activeElement).toBe(from)
    }
  })
})
