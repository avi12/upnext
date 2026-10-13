import { StrictMode, type ReactNode } from 'react'
import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import { MemoryRouter, useLocation } from 'react-router'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import { ToastContext, type ToastApi } from '../components/useToast'
import { SettingsPage } from '../pages/SettingsPage'
import { useSignInReturn } from './useSignInReturn'

afterEach(() => vi.restoreAllMocks())

function fakeToast() {
  return { push: vi.fn(), success: vi.fn(), error: vi.fn(), info: vi.fn() } as unknown as ToastApi & { success: ReturnType<typeof vi.fn>; error: ReturnType<typeof vi.fn> }
}

/** As main.tsx renders the app: in StrictMode, which runs a page's first effects twice in a development build. */
function around(toast: ToastApi, at: string, children: ReactNode) {
  return (
    <StrictMode>
      <QueryClientProvider client={new QueryClient()}>
        <ToastContext.Provider value={toast}>
          <MemoryRouter initialEntries={[at]}>{children}</MemoryRouter>
        </ToastContext.Provider>
      </QueryClientProvider>
    </StrictMode>
  )
}

function Page() {
  useSignInReturn({ steam: 'Your SteamID is saved', gog: 'Playtime comes from GOG on the next sync', gogErrors: true })
  return <p data-testid="address">{useLocation().search}</p>
}

describe('useSignInReturn', () => {
  it('back from Steam, Settings says "Signed in with Steam" once in a development build (B-WEB-7)', () => {
    vi.spyOn(api, 'settings').mockReturnValue(new Promise(() => {}))
    vi.spyOn(api, 'players').mockReturnValue(new Promise(() => {}))
    const toast = fakeToast()
    render(around(toast, '/settings?steam=ok', <SettingsPage />))
    expect(toast.success).toHaveBeenCalledTimes(1)
    expect(toast.success).toHaveBeenCalledWith('Signed in with Steam', 'Your SteamID is saved')
  })

  it('says a failed GOG login once, with the reason, and takes the parameters out of the address', async () => {
    const toast = fakeToast()
    render(around(toast, '/settings?gog=error&reason=Denied&tab=x', <Page />))
    expect(toast.error).toHaveBeenCalledTimes(1)
    expect(toast.error).toHaveBeenCalledWith("GOG login didn't work", 'Denied')
    await waitFor(() => expect(screen.getByTestId('address').textContent).toBe('?tab=x'))
  })
})
