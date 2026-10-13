import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { verdictsContract } from '../../test/verdictsContract'
import { api, ApiError } from '../../api'
import type { GameView, MyVerdict, Settings, VerdictConsent, VerdictForm } from '../../types'
import { BacklogVerdict } from './BacklogVerdict'

const game = { id: 7, name: 'Hollow Pines' } as GameView

const consent: VerdictConsent = {
  version: 1,
  title: 'Your verdict will be public.',
  paragraphs: ['Anyone can read it.'],
  accept: 'Publish my verdicts under CC0',
  decline: 'Not now',
  text_box_line: 'Public, CC0: anyone can read and reuse what you type.',
  licence: 'CC0-1.0',
  licence_url: 'https://creativecommons.org/publicdomain/zero/1.0/',
  terms_url: 'http://127.0.0.1:9/terms',
  accepted: null,
  accepted_on: null,
  current: false,
}

const sent: MyVerdict = {
  title_id: 7,
  name: 'Hollow Pines',
  answers: { outcome: 'finished', worth: 'yes' },
  steam_appid: 620,
  gog_id: null,
  remote_id: '01JA0000000000000000000000',
  state: 'sent',
  retry_at: null,
  error: null,
  sent_at: 1_790_000_000,
  updated_at: 1_790_000_000,
}

function form(over: Partial<VerdictForm> = {}): VerdictForm {
  return {
    title_id: 7,
    name: 'Hollow Pines',
    cant_send: null,
    answers: { outcome: 'finished', hours: '10_20' },
    from: { outcome: 'mark', hours: 'playtime' },
    problems: [],
    mine: null,
    outdated: false,
    service: true,
    consent,
    ...over,
  }
}

function setup(f: VerdictForm, props: { offer?: boolean; onOfferDone?: () => void } = {}) {
  vi.spyOn(api, 'verdictForm').mockResolvedValue(f)
  vi.spyOn(api, 'verdictContract').mockResolvedValue(verdictsContract)
  vi.spyOn(api, 'settings').mockResolvedValue({ verdicts_ask: true } as Settings)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <BacklogVerdict game={game} {...props} />
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

describe('Backlog verdicts on the game page', () => {
  afterEach(() => vi.restoreAllMocks())

  it('fills the form from your mark, asks for consent before the first send, and sends', async () => {
    const accept = vi.spyOn(api, 'acceptVerdictConsent').mockResolvedValue({ ...consent, current: true, accepted: 1 })
    const send = vi.spyOn(api, 'sendVerdict').mockResolvedValue({ verdict: sent, message: 'Sent. It shows with the next daily update.' })
    const user = setup(form())
    await user.click(await screen.findByRole('button', { name: 'Write a backlog verdict' }))
    expect(await screen.findByText('from your mark')).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Finished it' }).getAttribute('aria-pressed')).toBe('true')
    // Worth it is needed before anything can be sent.
    expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
    await user.click(within(screen.getByRole('group', { name: /Worth it/ })).getByRole('button', { name: 'Yes' }))
    await user.click(screen.getByRole('button', { name: 'Send' }))

    // The consent screen: Not now sends nothing.
    expect(await screen.findByText('Your verdict will be public.')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Not now' }))
    expect(send).not.toHaveBeenCalled()
    await user.click(screen.getByRole('button', { name: 'Send' }))
    await user.click(await screen.findByRole('button', { name: 'Publish my verdicts under CC0' }))
    await waitFor(() => expect(accept).toHaveBeenCalledWith(1))
    await waitFor(() => expect(send).toHaveBeenCalledWith(7, expect.objectContaining({ outcome: 'finished', worth: 'yes', hours: '10_20', stopped_for: [] })))
    expect(await screen.findByText('Sent. It shows with the next daily update.')).toBeTruthy()
  })

  it('refuses a link as you type, and shows what the service says is wrong beside the field', async () => {
    const send = vi
      .spyOn(api, 'sendVerdict')
      .mockRejectedValue(new ApiError(400, 'the verdict has problems', [{ field: 'hours', why: 'pick an answer from the list' }]))
    const user = setup(form({ answers: { outcome: 'finished', worth: 'yes' }, consent: { ...consent, current: true } }))
    await user.click(await screen.findByRole('button', { name: 'Write a backlog verdict' }))
    const box = await screen.findByRole('textbox', { name: /anything else/i })
    expect(screen.getByText('Public, CC0: anyone can read and reuse what you type.')).toBeTruthy()
    await user.type(box, 'see www.example.com')
    expect(screen.getByText('It has a link or a web address; take it out to send.')).toBeTruthy()
    expect((screen.getByRole('button', { name: 'Send' }) as HTMLButtonElement).disabled).toBe(true)
    await user.clear(box)
    await user.click(screen.getByRole('button', { name: 'Send' }))
    await waitFor(() => expect(send).toHaveBeenCalled())
    expect(await screen.findByRole('alert')).toBeTruthy()
    expect(screen.getByText('pick an answer from the list')).toBeTruthy()
  })

  it('shows your sent verdict and takes it down', async () => {
    const withdraw = vi.spyOn(api, 'withdrawVerdict').mockResolvedValue({ verdict: { ...sent, state: 'withdrawn' } })
    const user = setup(form({ mine: sent, answers: sent.answers }))
    expect(await screen.findByText(/Your verdict:/)).toBeTruthy()
    expect(screen.getByRole('button', { name: 'Edit' })).toBeTruthy()
    await user.click(screen.getByRole('button', { name: 'Take down' }))
    await waitFor(() => expect(withdraw).toHaveBeenCalledWith(7))
    expect(await screen.findByText(/left out of the next daily update/)).toBeTruthy()
  })

  it('offers a verdict after a mark, and stops asking when told', async () => {
    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue({ verdicts_ask: false } as Settings)
    const done = vi.fn()
    const user = setup(form(), { offer: true, onOfferDone: done })
    expect(await screen.findByText('Tell other backlog players how it went?')).toBeTruthy()
    await user.click(screen.getByRole('button', { name: "Don't ask again" }))
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_ask: false }))
    await waitFor(() => expect(done).toHaveBeenCalled())
  })

  it("says why a game can't have a verdict", async () => {
    setup(form({ cant_send: 'Verdicts are for games with a Steam or GOG page; this one has neither.' }))
    expect(await screen.findByText(/has neither/)).toBeTruthy()
    expect(screen.queryByRole('button', { name: 'Write a backlog verdict' })).toBeNull()
  })
})
