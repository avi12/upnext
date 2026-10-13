import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { MyVerdict, Settings, VerdictConsent, VerdictsOverview } from '../../types'
import { ToastProvider } from '../Toast'
import { VerdictsRow } from './VerdictsRow'

const verdict = { title_id: 7, name: 'Hollow Pines', state: 'sent', sent_at: 1_790_000_000 } as MyVerdict

function setup(settings: Partial<Settings>, overview: Partial<VerdictsOverview>) {
  vi.spyOn(api, 'settings').mockResolvedValue({
    verdicts_url: null,
    verdicts_url_source: null,
    verdicts_ask: true,
    listen: { this_computer: true },
    ...settings,
  } as Settings)
  vi.spyOn(api, 'verdicts').mockResolvedValue({ service: true, has_key: true, consent: {} as VerdictConsent, verdicts: [verdict], ...overview })
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <VerdictsRow />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

describe('Settings, backlog verdicts', () => {
  afterEach(() => vi.restoreAllMocks())

  it('sets the service address and stops the game page offering verdicts', async () => {
    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue({ verdicts_url: 'http://den.lan:8788', verdicts_ask: true, listen: { this_computer: true } } as Settings)
    const user = setup({}, { verdicts: [], has_key: false, service: false })
    expect(await screen.findByText('· 0 sent')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Change' }))
    await user.type(screen.getByLabelText('Service address'), 'http://den.lan:8788{Enter}')
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_url: 'http://den.lan:8788' }))
    await user.click(screen.getByRole('checkbox'))
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_ask: false }))
  })

  it('shows the address on another device without a field to change it', async () => {
    const user = setup({ verdicts_url: 'http://den.lan:8788', verdicts_url_source: 'app', listen: { this_computer: false } as Settings['listen'] }, {})
    await user.click(await screen.findByRole('button', { name: 'Change' }))
    expect(await screen.findByText('http://den.lan:8788')).toBeTruthy()
    expect(screen.queryByLabelText('Service address')).toBeNull()
  })

  it('takes every verdict down only after asking', async () => {
    const all = vi.spyOn(api, 'withdrawAllVerdicts').mockResolvedValue({ withdrawn: 1, forgot_key: true })
    const user = setup({ verdicts_url: 'http://den.lan:8788', verdicts_url_source: 'app' }, {})
    expect(await screen.findByText('· 1 sent')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Change' }))
    await user.click(await screen.findByRole('button', { name: 'Take down all my verdicts' }))
    expect(all).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Take them all down' }))
    await waitFor(() => expect(all).toHaveBeenCalled())
    expect(await screen.findByText('1 verdict; your key is forgotten.')).toBeTruthy()
  })
})
