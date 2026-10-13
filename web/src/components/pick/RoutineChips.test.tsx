import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { MemoryRouter } from 'react-router'
import { useState } from 'react'
import { api } from '../../api'
import type { Routine } from '../../types'
import { chosenRoutine, type RoutineChoice } from '../../lib/routines'
import { RoutineChips } from './RoutineChips'

const routine = (id: number, name: string, change: Partial<Routine> = {}): Routine => ({
  id,
  name,
  days: [],
  times: [],
  who: null,
  place: null,
  input: null,
  minutes: null,
  note: null,
  ...change,
})

const ROUTINES = [routine(1, 'Weeknight wind-down', { who: 'solo', minutes: 60 }), routine(2, 'Saturday co-op', { who: 'friends', days: ['sat'] })]

function mount(now: number | null, routines = ROUTINES) {
  vi.spyOn(api, 'routines').mockResolvedValue({ routines, now })
  const seen: RoutineChoice[] = []
  function Page() {
    const [choice, setChoice] = useState<RoutineChoice>(undefined)
    seen.push(choice)
    return <RoutineChips choice={choice} onChoose={setChoice} />
  }
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  const { container } = render(
    <QueryClientProvider client={client}>
      <MemoryRouter>
        <Page />
      </MemoryRouter>
    </QueryClientProvider>,
  )
  return { seen, container }
}

describe('Routine chips on the Pick page', () => {
  afterEach(() => vi.restoreAllMocks())

  it('chooses the routine that fits now until another is tapped, and none when the chosen one is tapped again', async () => {
    mount(2)
    const coop = await screen.findByRole('button', { name: /Saturday co-op/ })
    expect(coop.getAttribute('aria-pressed')).toBe('true')
    expect(coop.textContent).toContain('now')
    expect(screen.getByText(/Picks fit “Saturday co-op”: Saturdays · with friends\./)).toBeTruthy()

    const user = userEvent.setup()
    const wind = screen.getByRole('button', { name: /Weeknight wind-down/ })
    await user.click(wind)
    expect(wind.getAttribute('aria-pressed')).toBe('true')
    expect(coop.getAttribute('aria-pressed')).toBe('false')

    await user.click(wind)
    expect(screen.getAllByRole('button').every((b) => b.getAttribute('aria-pressed') === 'false')).toBe(true)
    expect(screen.getByText(/No routine/)).toBeTruthy()
  })

  it('shows nothing without routines', async () => {
    const { container } = mount(null, [])
    await vi.waitFor(() => expect(api.routines).toHaveBeenCalled())
    expect(container.textContent).toBe('')
  })

  it('follows now, a chosen id, or none, and forgets a deleted routine', () => {
    expect(chosenRoutine(ROUTINES, 1, undefined)?.name).toBe('Weeknight wind-down')
    expect(chosenRoutine(ROUTINES, 1, 2)?.name).toBe('Saturday co-op')
    expect(chosenRoutine(ROUTINES, 1, null)).toBeNull()
    expect(chosenRoutine(ROUTINES, null, 9)).toBeNull()
  })
})
