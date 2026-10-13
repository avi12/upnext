import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import { ToastContext, type ToastApi } from '../useToast'
import { RoutinesSection } from './Routines'

function mount() {
  const toast: ToastApi = { push: vi.fn(), success: vi.fn(), error: vi.fn(), info: vi.fn() }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <ToastContext.Provider value={toast}>
          <RoutinesSection />
        </ToastContext.Provider>
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return toast
}

describe('Routines on the Profile', () => {
  afterEach(() => vi.restoreAllMocks())

  it('makes a routine from chips and says which one fits now', async () => {
    const saved = {
      id: 3,
      name: 'Friday with friends',
      days: ['fri' as const],
      times: ['evenings' as const],
      who: 'friends' as const,
      place: 'desk' as const,
      input: 'keyboard_mouse' as const,
      minutes: 120,
      note: null,
    }
    const list = vi.spyOn(api, 'routines').mockResolvedValueOnce({ routines: [], now: null }).mockResolvedValue({ routines: [saved], now: 3 })
    const add = vi.spyOn(api, 'addRoutine').mockResolvedValue(saved)
    const toast = mount()

    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: 'Add your first routine' }))
    await user.type(screen.getByLabelText('Name'), '  Friday with friends ')
    await user.click(screen.getByRole('button', { name: 'Fri' }))
    await user.click(screen.getByRole('button', { name: 'Evenings' }))
    await user.click(screen.getByRole('button', { name: 'With friends' }))
    await user.click(screen.getByRole('button', { name: 'At a desk' }))
    await user.click(screen.getByRole('button', { name: 'Keyboard and mouse' }))
    await user.click(screen.getByRole('button', { name: 'two hours' }))
    await user.click(screen.getByRole('button', { name: 'Save' }))

    expect(add).toHaveBeenCalledWith({
      name: 'Friday with friends',
      days: ['fri'],
      times: ['evenings'],
      who: 'friends',
      place: 'desk',
      input: 'keyboard_mouse',
      minutes: 120,
      note: null,
    })
    expect(toast.success).toHaveBeenCalledWith('Added “Friday with friends”')
    expect(await screen.findByText('Friday with friends')).toBeTruthy()
    expect(screen.getByText('fits now')).toBeTruthy()
    expect(screen.getByText(/Friday evenings · with friends · at a desk · keyboard and mouse · 2/)).toBeTruthy()
    expect(list).toHaveBeenCalledTimes(2)
  })

  it('takes a chosen single chip back off when tapped again', async () => {
    vi.spyOn(api, 'routines').mockResolvedValue({ routines: [], now: null })
    mount()
    const user = userEvent.setup()
    await user.click(await screen.findByRole('button', { name: 'Add your first routine' }))
    const couch = screen.getByRole('button', { name: 'On the couch' })
    await user.click(couch)
    expect(couch.getAttribute('aria-pressed')).toBe('true')
    await user.click(couch)
    expect(couch.getAttribute('aria-pressed')).toBe('false')
    expect((screen.getByRole('button', { name: 'Save' }) as HTMLButtonElement).disabled).toBe(true)
  })
})
