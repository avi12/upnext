import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import { SOURCES } from '../../lib/format'
import type { Config } from '../../types'
import { AskForm } from './AskForm'
import { storesLabel } from './text'

describe('AskForm', () => {
  afterEach(() => vi.restoreAllMocks())

  it('takes no more than the 2,000 characters the server does', async () => {
    vi.spyOn(api, 'tasteSources').mockResolvedValue({ library: { games: 0 } } as Awaited<ReturnType<typeof api.tasteSources>>)
    const onDraft = vi.fn()
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AskForm
            prompt=""
            onDraft={onDraft}
            onRoll={() => {}}
            onDice={() => {}}
            pending={false}
            count={3}
            setCount={() => {}}
            sources={[...SOURCES]}
            toggleSource={() => {}}
            config={undefined}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    )
    const box = screen.getByLabelText('What are you in the mood for?') as HTMLInputElement
    expect(box.maxLength).toBe(2000)

    const user = userEvent.setup()
    await user.click(box)
    await user.paste('x'.repeat(2500))
    expect(onDraft).toHaveBeenLastCalledWith('x'.repeat(2000))
  })

  it('says an API key backend has no key yet, not the command-line message', () => {
    vi.spyOn(api, 'tasteSources').mockResolvedValue({ library: { games: 0 } } as Awaited<ReturnType<typeof api.tasteSources>>)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const config = {
      claude_available: false,
      llm_backend: 'api-key',
      llm_error: 'no Anthropic API key: add one on the Settings page, with `upnext config set anthropic_api_key`, or set ANTHROPIC_API_KEY',
    } as Config
    const { container } = render(
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AskForm
            prompt=""
            onDraft={() => {}}
            onRoll={() => {}}
            onDice={() => {}}
            pending={false}
            count={3}
            setCount={() => {}}
            sources={[...SOURCES]}
            toggleSource={() => {}}
            config={config}
          />
        </MemoryRouter>
      </QueryClientProvider>,
    )
    expect(container.textContent).toMatch(/Anthropic API key is chosen, with no key saved yet\. Set one up/)
    expect(container.textContent).not.toMatch(/upnext config set/)
  })

  it('says which libraries picks come from, not which stores', () => {
    expect(storesLabel([...SOURCES])).toBe('Any library')
    expect(storesLabel(['steam'])).toBe('From Steam')
    expect(storesLabel(['steam', 'gog'])).toBe('From Steam and GOG')
  })

  it('curates rather than rolls, and sorts its cards while it works', () => {
    vi.spyOn(api, 'tasteSources').mockResolvedValue({ library: { games: 0 } } as Awaited<ReturnType<typeof api.tasteSources>>)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const form = (pending: boolean) => (
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AskForm
            prompt="A quiet hour"
            onDraft={() => {}}
            onRoll={() => {}}
            onDice={() => {}}
            pending={pending}
            count={3}
            setCount={() => {}}
            sources={[...SOURCES]}
            toggleSource={() => {}}
            config={undefined}
          />
        </MemoryRouter>
      </QueryClientProvider>
    )
    const { rerender } = render(form(false))
    const button = screen.getByRole('button', { name: 'Curate' })
    expect(button.getAttribute('title')).toBe('Curate (Ctrl+Enter)')
    expect(button.querySelector('svg.curating')).toBeNull()
    rerender(form(true))
    const busy = screen.getByRole('button', { name: 'Curating…' })
    expect(busy.querySelector('svg.curating')).not.toBeNull()
  })

  it('has a dice button beside Curate that rolls a random game, and Curate stays Curate while it does', async () => {
    vi.spyOn(api, 'tasteSources').mockResolvedValue({ library: { games: 0 } } as Awaited<ReturnType<typeof api.tasteSources>>)
    const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
    const onDice = vi.fn()
    const onRoll = vi.fn()
    const form = (dicing: boolean) => (
      <QueryClientProvider client={client}>
        <MemoryRouter>
          <AskForm
            prompt="A quiet hour"
            onDraft={() => {}}
            onRoll={onRoll}
            onDice={onDice}
            pending={dicing}
            dicing={dicing}
            count={3}
            setCount={() => {}}
            sources={[...SOURCES]}
            toggleSource={() => {}}
            config={undefined}
          />
        </MemoryRouter>
      </QueryClientProvider>
    )
    const { rerender } = render(form(false))
    const dice = screen.getByRole('button', { name: 'Roll the dice' })
    expect(dice.getAttribute('title')).toMatch(/at random, with no AI/)
    await userEvent.setup().click(dice)
    expect(onDice).toHaveBeenCalledOnce()
    expect(onRoll).not.toHaveBeenCalled()
    rerender(form(true))
    expect((screen.getByRole('button', { name: 'Roll the dice' }) as HTMLButtonElement).disabled).toBe(true)
    expect(screen.getByRole('button', { name: 'Roll the dice' }).querySelector('svg.dicing')).not.toBeNull()
    const curate = screen.getByRole('button', { name: 'Curate' }) as HTMLButtonElement
    expect(curate.disabled).toBe(true)
    expect(curate.querySelector('svg.curating')).toBeNull()
  })
})
