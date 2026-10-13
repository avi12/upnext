import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../../api'
import type { TasteQuestions as Questions, TasteResponse } from '../../types'
import { ToastProvider } from '../Toast'
import { TasteQuestions } from './TasteQuestions'

function mount(props: Partial<Parameters<typeof TasteQuestions>[0]> = {}) {
  vi.spyOn(api, 'taste').mockResolvedValue({ favourites: [], statements: [], habits: [] } as unknown as TasteResponse)
  vi.spyOn(api, 'tasteQuestions').mockResolvedValue({
    most_played: [],
    few_played: false,
    loves: [],
    hard_nos: [],
    diary: { on: false, steam: true },
  } as Questions)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <TasteQuestions onFinish={vi.fn()} {...props} />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

describe('The quick questions', () => {
  afterEach(() => vi.restoreAllMocks())

  it('ask when and where as well as who with, and say what personal means', async () => {
    const user = mount()
    await user.click(await screen.findByRole('button', { name: 'Skip this' }))
    expect(screen.getByRole('group', { name: 'When you usually play' })).toBeTruthy()
    expect(screen.getByRole('group', { name: 'Where you play' })).toBeTruthy()
    expect(screen.getByRole('group', { name: 'Who you play with' })).toBeTruthy()
    expect(screen.getAllByText('Optional · personal')).toHaveLength(2)
    expect(screen.getByText(/it goes to your AI with each pick/)).toBeTruthy()
    expect(screen.getByText(/The diary itself stays on this computer; the habits it finds go to your AI with your picks/)).toBeTruthy()
    expect(screen.queryByText(/It stays on this computer/)).toBeNull()
  })
})

describe('The quick questions in the setup wizard', () => {
  afterEach(() => vi.restoreAllMocks())

  it('have one Back and one way on per page, with Skip only before the last', async () => {
    const back = vi.fn()
    const skip = vi.fn()
    const end = vi.fn()
    const user = mount({ onEnd: end, onBack: back, onSkip: skip })
    const first = await screen.findByRole('button', { name: 'Next' })
    expect(screen.getAllByRole('button', { name: 'Back' })).toHaveLength(1)
    expect(screen.getByRole('button', { name: 'Skip this step' })).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Back' }))
    expect(back).toHaveBeenCalledTimes(1)

    await user.click(first)
    expect(screen.getAllByRole('button', { name: 'Back' })).toHaveLength(1)
    expect(screen.queryByRole('button', { name: 'Skip this step' })).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Next' }))
    expect(end).toHaveBeenCalledTimes(1)
    expect(skip).not.toHaveBeenCalled()
  })
})
