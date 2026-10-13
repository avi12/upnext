import { Suspense, lazy } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen } from '@testing-library/react'
import { ErrorBoundary } from './ErrorBoundary'
import { page, reloadOnce, resetReload, scriptMissing } from '../lib/reload'

/** What Chromium says when a page's script is gone after an upgrade. */
const GONE = 'Failed to fetch dynamically imported module: http://127.0.0.1:8787/assets/SettingsPage-0ld0ld00.js'

function Broken(): never {
  throw new Error('a page that breaks')
}

/** A page that loads later, whose script the server no longer has. */
function stalePage() {
  return lazy(() => Promise.reject(new TypeError(GONE)))
}

beforeEach(() => {
  window.sessionStorage.clear()
  resetReload()
  vi.spyOn(console, 'error').mockImplementation(() => {})
})
afterEach(() => vi.restoreAllMocks())

describe('ErrorBoundary', () => {
  it('shows what failed and a Reload button instead of a blank page', () => {
    const reload = vi.spyOn(page, 'reload').mockImplementation(() => {})
    render(
      <ErrorBoundary>
        <Broken />
      </ErrorBoundary>,
    )
    expect(screen.getByRole('alert').textContent).toContain('Something went wrong on this page.')
    expect(screen.getByText('a page that breaks')).toBeTruthy()
    expect(reload).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }))
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('loads the page again once when a page’s script is gone after an upgrade', async () => {
    const reload = vi.spyOn(page, 'reload').mockImplementation(() => {})
    const Settings = stalePage()
    render(
      <ErrorBoundary>
        <Suspense fallback={null}>
          <Settings />
        </Suspense>
      </ErrorBoundary>,
    )
    expect(await screen.findByText('Loading the new version of UpNext…')).toBeTruthy()
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('says UpNext was updated, rather than reloading again, when the script is still gone', async () => {
    const reload = vi.spyOn(page, 'reload').mockImplementation(() => {})
    // The reload a moment ago, as the page remembers it.
    expect(reloadOnce()).toBe(true)
    resetReload()
    reload.mockClear()
    const Settings = stalePage()
    render(
      <ErrorBoundary>
        <Suspense fallback={null}>
          <Settings />
        </Suspense>
      </ErrorBoundary>,
    )
    const alert = await screen.findByRole('alert')
    expect(alert.textContent).toContain('UpNext was updated while this page was open')
    expect(reload).not.toHaveBeenCalled()
    fireEvent.click(screen.getByRole('button', { name: 'Reload' }))
    expect(reload).toHaveBeenCalledTimes(1)
  })

  it('clears the failure when another page opens', () => {
    const { rerender } = render(
      <ErrorBoundary resetKey="/settings">
        <Broken />
      </ErrorBoundary>,
    )
    expect(screen.getByRole('alert')).toBeTruthy()
    rerender(
      <ErrorBoundary resetKey="/library">
        <p>The library</p>
      </ErrorBoundary>,
    )
    expect(screen.queryByRole('alert')).toBeNull()
    expect(screen.getByText('The library')).toBeTruthy()
  })
})

describe('reloadOnce', () => {
  it('reloads once a minute at most', () => {
    const reload = vi.spyOn(page, 'reload').mockImplementation(() => {})
    expect(reloadOnce(1_000_000)).toBe(true)
    resetReload()
    expect(reloadOnce(1_000_000 + 30_000)).toBe(false)
    expect(reloadOnce(1_000_000 + 61_000)).toBe(true)
    expect(reload).toHaveBeenCalledTimes(2)
  })

  it('never reloads when it couldn’t remember doing so', () => {
    const reload = vi.spyOn(page, 'reload').mockImplementation(() => {})
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('storage is off')
    })
    expect(reloadOnce()).toBe(false)
    expect(reload).not.toHaveBeenCalled()
  })

  it('tells a missing script from other failures, in each browser’s words', () => {
    expect(scriptMissing(new TypeError(GONE))).toBe(true)
    expect(scriptMissing(new TypeError('Importing a module script failed.'))).toBe(true)
    expect(scriptMissing(new TypeError('error loading dynamically imported module: http://den.lan:8787/assets/x.js'))).toBe(true)
    expect(scriptMissing(new Error('Unable to preload CSS for /assets/x.css'))).toBe(true)
    expect(scriptMissing(new Error('a page that breaks'))).toBe(false)
  })
})
