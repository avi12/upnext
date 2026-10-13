import { afterEach, describe, expect, it, vi } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { api } from '../api'
import type { HideScope, Player } from '../types'
import { ToastProvider } from './Toast'
import { StatusMenu } from './StatusMenu'

function player(id: number, name: string): Player {
  return { id, name, colour: '#4f8cff', has_pin: false, created_at: 1790000000, default: id === 1 }
}

function show(game: { hidden_scope?: HideScope | null; own_copy?: boolean }, players = [player(1, 'Alex'), player(2, 'Sam')]) {
  vi.spyOn(api, 'players').mockResolvedValue({ players, current: 1, bound: false })
  vi.spyOn(api, 'settings').mockRejectedValue(new Error('not needed'))
  const onHidden = vi.fn()
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <ToastProvider>
        <StatusMenu
          game={{ override: null, override_at: null, kind: 'ending' }}
          onChange={() => {}}
          hide={{ id: 5, name: 'Hades', hidden_scope: game.hidden_scope ?? null, own_copy: game.own_copy ?? false }}
          onHidden={onHidden}
        />
      </ToastProvider>
    </QueryClientProvider>,
  )
  return { onHidden }
}

describe('Hide this game', () => {
  afterEach(() => vi.restoreAllMocks())

  it('hides your own copy from everyone else and says who still sees it', async () => {
    const hide = vi.spyOn(api, 'hideGame').mockResolvedValue({ scope: 'others', still_seen_by: ['Sam'], through_shared: [] })
    const { onHidden } = show({ own_copy: true })
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hide this game/ }))
    expect(screen.getByText(/Hidden in UpNext only. Anyone with this computer’s files can still see it./)).toBeTruthy()
    fireEvent.click(await screen.findByRole('button', { name: /From everyone else/ }))
    await waitFor(() => expect(hide).toHaveBeenCalledWith(5, 'others'))
    await screen.findByText('Sam has their own copy, so they’ll still see it.')
    expect(onHidden).toHaveBeenCalledWith('others')
  })

  it("can't hide someone else's copy from the others", async () => {
    show({ own_copy: false })
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hide this game/ }))
    expect((await screen.findByRole('button', { name: /From everyone else/ }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /^From everyone\s*Only/ }) as HTMLButtonElement).disabled).toBe(true)
    expect((screen.getByRole('button', { name: /Just from me/ }) as HTMLButtonElement).disabled).toBe(false)
  })

  it('unhides a game you hid', async () => {
    const unhide = vi.spyOn(api, 'unhideGame').mockResolvedValue(undefined)
    show({ own_copy: true, hidden_scope: 'others' })
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hidden from everyone else/ }))
    fireEvent.click(await screen.findByRole('button', { name: /Unhide/ }))
    await waitFor(() => expect(unhide).toHaveBeenCalledWith(5))
  })

  it('hidden just from you, says nothing about who else still sees it', async () => {
    vi.spyOn(api, 'hideGame').mockResolvedValue({ scope: 'me', still_seen_by: ['Sam'], through_shared: [] })
    show({ own_copy: true })
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hide this game/ }))
    fireEvent.click(await screen.findByRole('button', { name: /Just from me/ }))
    await screen.findByText('Hades: hidden from you')
    expect(screen.queryByText(/still see/)).toBeNull()
  })

  it('names who sees it through someone else’s shared account', async () => {
    vi.spyOn(api, 'hideGame').mockResolvedValue({ scope: 'others', still_seen_by: ['Kit'], through_shared: ['Kit'] })
    show({ own_copy: true })
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hide this game/ }))
    fireEvent.click(await screen.findByRole('button', { name: /From everyone else/ }))
    await screen.findByText('Kit still sees it through someone else’s shared account.')
  })

  it('with one player, hides only from yourself', async () => {
    const hide = vi.spyOn(api, 'hideGame').mockResolvedValue({ scope: 'me', still_seen_by: [], through_shared: [] })
    show({ own_copy: true }, [player(1, 'Alex')])
    fireEvent.click(screen.getByRole('button', { name: /Mark as/ }))
    fireEvent.click(await screen.findByRole('menuitem', { name: /Hide this game/ }))
    expect(screen.queryByRole('button', { name: /From everyone/ })).toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Hide it/ }))
    await waitFor(() => expect(hide).toHaveBeenCalledWith(5, 'me'))
  })
})
