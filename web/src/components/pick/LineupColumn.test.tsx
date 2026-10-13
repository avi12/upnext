import { describe, expect, it, vi } from 'vitest'
import { render, screen } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import type { OwnedPick } from '../../types'
import { MOCK_GAME } from '../../test/mockGame'
import { ToastProvider } from '../Toast'
import { LineupColumn } from './LineupColumn'

const PICK = { id: MOCK_GAME.id, appid: MOCK_GAME.appid, name: MOCK_GAME.name, reason: 'Calm and short.', slot: 0, feedback: null, game: MOCK_GAME } as OwnedPick

describe('A pick card’s "It’s too ___"', () => {
  it('looks like a field, with examples, and Swap waits for a word', async () => {
    const onToo = vi.fn()
    const client = new QueryClient()
    render(
      <QueryClientProvider client={client}>
        <ToastProvider>
          <LineupColumn pick={PICK} number={1} compared={false} onCompare={vi.fn()} onOpen={vi.fn()} showLinux={false} busy={false} sifting={null} onAct={vi.fn()} onToo={onToo} />
        </ToastProvider>
      </QueryClientProvider>,
    )
    const field = screen.getByLabelText("It's too") as HTMLInputElement
    expect(field.value).toBe('')
    expect(field.placeholder).toBe('long, slow, grindy…')
    expect(field.className).toMatch(/border /)
    const swap = screen.getByRole('button', { name: 'Swap' }) as HTMLButtonElement
    expect(swap.disabled).toBe(true)
    const user = userEvent.setup()
    await user.type(field, 'grindy')
    expect(swap.disabled).toBe(false)
    await user.click(swap)
    expect(onToo).toHaveBeenCalledWith('grindy')
  })
})

describe('A pick card’s thumbs', () => {
  function mount(rating: OwnedPick['rating']) {
    const onRate = vi.fn()
    const onAct = vi.fn()
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ToastProvider>
          <LineupColumn pick={{ ...PICK, rating }} number={1} compared={false} onCompare={vi.fn()} onOpen={vi.fn()} showLinux={false} busy={false} sifting={null} onAct={onAct} onToo={vi.fn()} onRate={onRate} />
        </ToastProvider>
      </QueryClientProvider>,
    )
    return { onRate, onAct, user: userEvent.setup() }
  }

  it('rates the pick, not the game, and asks no why for a good pick', async () => {
    const { onRate, onAct, user } = mount(null)
    expect(screen.getByRole('group', { name: 'How well this pick fits your request' })).toBeTruthy()
    expect(screen.queryByText('Why? If you like:')).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Good pick' }))
    expect(onRate).toHaveBeenCalledWith('good')
    expect(onAct).not.toHaveBeenCalled()
  })

  it('takes a rating back when its thumb is tapped again', async () => {
    const { onRate, user } = mount({ verdict: 'good', reason: null, at: 1 })
    const good = screen.getByRole('button', { name: 'Good pick' })
    expect(good.getAttribute('aria-pressed')).toBe('true')
    await user.click(good)
    expect(onRate).toHaveBeenCalledWith(null)
  })

  it('asks why after Not a fit, in one tap you can skip or take back', async () => {
    const { onRate, user } = mount({ verdict: 'not_a_fit', reason: 'wrong_length', at: 1 })
    expect(screen.getByRole('button', { name: 'Not a fit' }).getAttribute('aria-pressed')).toBe('true')
    const why = screen.getByRole('group', { name: "Why it wasn't a fit" })
    expect(why.textContent).toContain('Wrong lengthWrong moodWrong genrePlayed out')
    await user.click(screen.getByRole('button', { name: 'Wrong mood' }))
    expect(onRate).toHaveBeenLastCalledWith('not_a_fit', 'wrong_mood')
    const length = screen.getByRole('button', { name: 'Wrong length' })
    expect(length.getAttribute('aria-pressed')).toBe('true')
    await user.click(length)
    expect(onRate).toHaveBeenLastCalledWith('not_a_fit', null)
  })
})

describe('A game drawn with the dice', () => {
  it('is one card with Another and Curate around this, and no compare, Not tonight, "It’s too" or thumbs', async () => {
    const onAnother = vi.fn()
    const onCurateAround = vi.fn()
    const onAct = vi.fn()
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ToastProvider>
          <LineupColumn
            pick={PICK}
            number={1}
            compared={false}
            onCompare={vi.fn()}
            onOpen={vi.fn()}
            showLinux={false}
            busy={false}
            sifting={null}
            onAct={onAct}
            onToo={vi.fn()}
            onRate={vi.fn()}
            dice={{ onAnother, onCurateAround }}
          />
        </ToastProvider>
      </QueryClientProvider>,
    )
    expect(screen.queryByLabelText('Compare')).toBeNull()
    expect(screen.queryByRole('button', { name: 'Not tonight' })).toBeNull()
    expect(screen.queryByLabelText("It's too")).toBeNull()
    expect(screen.queryByRole('group', { name: 'How well this pick fits your request' })).toBeNull()
    const user = userEvent.setup()
    await user.click(screen.getByRole('button', { name: 'Another' }))
    expect(onAnother).toHaveBeenCalledOnce()
    await user.click(screen.getByRole('button', { name: 'Curate around this' }))
    expect(onCurateAround).toHaveBeenCalledOnce()
    // What it tells UpNext about the game still counts.
    await user.click(screen.getByRole('button', { name: 'Never recommend this' }))
    expect(onAct).toHaveBeenCalledWith('not_for_me')
  })
})
