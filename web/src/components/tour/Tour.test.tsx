import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { api } from '../../api'
import type { Config, SetupState } from '../../types'
import { Tour } from './Tour'

function show(aiReady: boolean) {
  vi.spyOn(api, 'setupState').mockResolvedValue({ tour_offer: true, tour: 'open' } as unknown as SetupState)
  vi.spyOn(api, 'config').mockResolvedValue({ claude_available: aiReady } as Config)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <Tour />
      </MemoryRouter>
    </QueryClientProvider>,
  )
}

describe('The tour’s offer line', () => {
  afterEach(() => vi.restoreAllMocks())

  it('offers the setup wizard too while no AI is set up', async () => {
    show(false)
    const setup = await screen.findByRole('link', { name: 'Run setup to add an AI' })
    expect(setup.getAttribute('href')).toBe('/setup?again=1')
    expect(screen.getByRole('button', { name: 'Show me around' })).toBeTruthy()
  })

  it('offers only the tour once an AI is set up', async () => {
    show(true)
    await screen.findByRole('button', { name: 'Show me around' })
    await new Promise((r) => setTimeout(r, 0))
    expect(screen.queryByRole('link', { name: 'Run setup to add an AI' })).toBeNull()
  })
})
