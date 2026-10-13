import { afterEach, describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor, within } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'
import { verdictsContract } from '../../test/verdictsContract'
import { MOCK_GAME } from '../../test/mockGame'
import { api, ApiError } from '../../api'
import type { GameCommunity, GameVerdicts, Job, OwnedPick, Settings, VerdictConsent } from '../../types'
import { ToastProvider } from '../Toast'
import { LineupColumn } from '../pick/LineupColumn'
import { CommunityVerdicts } from './CommunityVerdicts'

const VERDICTS: GameVerdicts = {
  n: 23,
  headline: 'Worth it on a backlog, say 18 of 23 players',
  lines: ['Clicks within the first hour', 'Fine in sessions under an hour'],
  counts_line: null,
  outcomes: [
    { outcome: 'finished', label: 'Finished: usually 10 to 20 h', count: 17 },
    { outcome: 'stopped', label: 'Stopped: mostly for the grind', count: 6 },
  ],
  texts: [1, 2, 3, 4].map((i) => ({
    id: `aaaaaaaaaaaaaaaaaaaaaaaaa${'bcde'[i - 1]}`,
    outcome: 'finished',
    about: 'Finished it · 10 to 20 hours · Worth it: Yes',
    text: `Runs are short (${i}).`,
    stop_note: null,
    month: '2026-10',
  })),
}

function community(over: Partial<GameCommunity> = {}): GameCommunity {
  return {
    on: true,
    chosen: true,
    service: true,
    licence: 'CC0-1.0',
    snapshot: { date: '2026-10-09', source: 'http://127.0.0.1:9', applied_at: 1, checked_at: 1, games: 4, verdicts: 35, matched: 2 },
    verdicts: VERDICTS,
    ...over,
  }
}

function show(c: GameCommunity) {
  vi.spyOn(api, 'gameVerdicts').mockResolvedValue(c)
  vi.spyOn(api, 'verdictContract').mockResolvedValue(verdictsContract)
  const client = new QueryClient({ defaultOptions: { queries: { retry: false }, mutations: { retry: false } } })
  render(
    <QueryClientProvider client={client}>
      <CommunityVerdicts game={MOCK_GAME} />
    </QueryClientProvider>,
  )
  return userEvent.setup()
}

afterEach(() => vi.restoreAllMocks())

describe("Other players' backlog verdicts on the game page", () => {
  it('shows the headline, what most say, how far people got and three texts, with where they came from', async () => {
    const user = show(community())
    expect(await screen.findByText('Worth it on a backlog, say 18 of 23 players')).toBeTruthy()
    expect(screen.getByText('Clicks within the first hour')).toBeTruthy()
    expect(screen.getByText('Finished: usually 10 to 20 h')).toBeTruthy()
    expect(screen.getByText('Stopped: mostly for the grind')).toBeTruthy()
    expect(screen.getAllByText(/^Runs are short/)).toHaveLength(3)
    await user.click(screen.getByRole('button', { name: 'More (1)' }))
    expect(screen.getAllByText(/^Runs are short/)).toHaveLength(4)
    const source = screen.getByText(/From 23 UpNext players, updated 9 October 2026\. Public under/)
    expect(within(source).getByRole('link', { name: 'CC0' })).toBeTruthy()
  })

  it('reports a text with a reason from the contract', async () => {
    const report = vi.spyOn(api, 'reportVerdict').mockResolvedValue({})
    const user = show(community())
    await screen.findByText('Runs are short (1).')
    await user.click(screen.getAllByRole('button', { name: 'Report' })[0])
    await user.click(await screen.findByRole('button', { name: 'Spam' }))
    await waitFor(() => expect(report).toHaveBeenCalledWith('aaaaaaaaaaaaaaaaaaaaaaaaab', 'spam'))
    expect(await screen.findByText(/^Reported\./)).toBeTruthy()
  })

  it('shows the consent screen before the first report, then sends it', async () => {
    const report = vi
      .spyOn(api, 'reportVerdict')
      .mockRejectedValueOnce(new ApiError(409, 'accept the consent screen first'))
      .mockResolvedValueOnce({})
    const consent: VerdictConsent = {
      version: 1,
      title: 'Your verdict will be public',
      paragraphs: ['Reports go with your anonymous verdicts key.'],
      accept: 'Publish',
      decline: 'Not now',
      text_box_line: '',
      licence: 'CC0-1.0',
      licence_url: 'https://creativecommons.org/publicdomain/zero/1.0/',
      terms_url: null,
      accepted: null,
      accepted_on: null,
      current: false,
    }
    vi.spyOn(api, 'verdictConsent').mockResolvedValue(consent)
    const accept = vi.spyOn(api, 'acceptVerdictConsent').mockResolvedValue({ ...consent, current: true })
    const user = show(community())
    await screen.findByText('Runs are short (1).')
    await user.click(screen.getAllByRole('button', { name: 'Report' })[0])
    await user.click(await screen.findByRole('button', { name: 'Spam' }))
    expect(await screen.findByText('Your verdict will be public')).toBeTruthy()
    expect(screen.queryByText(/accept the consent screen first/)).toBeNull()
    await user.click(screen.getByRole('button', { name: 'Publish' }))
    await waitFor(() => expect(accept).toHaveBeenCalledWith(1))
    await waitFor(() => expect(report).toHaveBeenCalledTimes(2))
    expect(await screen.findByText(/^Reported\./)).toBeTruthy()
  })

  it('gives counts, not lines, under five verdicts', async () => {
    show(community({ verdicts: { ...VERDICTS, n: 3, lines: [], counts_line: "2 say it's worth it, 1 says no", texts: [] } }))
    expect(await screen.findByText("2 say it's worth it, 1 says no.")).toBeTruthy()
    expect(screen.getByText(/From 3 UpNext players/)).toBeTruthy()
  })

  it('shows nothing while off once someone chose, and offers them once otherwise', async () => {
    show(community({ on: false, verdicts: null }))
    await waitFor(() => expect(api.gameVerdicts).toHaveBeenCalled())
    expect(screen.queryByRole('group')).toBeNull()
    vi.restoreAllMocks()
    document.body.innerHTML = ''

    const save = vi.spyOn(api, 'updateSettings').mockResolvedValue({ verdicts_receive: true } as Settings)
    const run = vi.spyOn(api, 'runTask').mockResolvedValue({} as Job)
    const user = show(community({ on: false, chosen: false, verdicts: null }))
    const note = await screen.findByRole('group', { name: "Other players' verdicts" })
    expect(within(note).getByText(/a daily download from the verdicts service that sends nothing about you/)).toBeTruthy()
    await user.click(within(note).getByRole('button', { name: 'Turn them on' }))
    await waitFor(() => expect(save).toHaveBeenCalledWith({ verdicts_receive: true }))
    expect(run).toHaveBeenCalledWith('verdicts')
    expect(await screen.findByText('On. Downloading them now.')).toBeTruthy()
  })
})

describe('A pick card’s backlog players line', () => {
  it('shows the line the server worked out for the request', () => {
    const pick = { id: 42, appid: 413150, name: 'Stardew Valley', reason: 'Calm.', slot: 0, feedback: null, game: MOCK_GAME } as OwnedPick
    render(
      <QueryClientProvider client={new QueryClient()}>
        <ToastProvider>
          <LineupColumn
            pick={{ ...pick, backlog: 'Backlog players: fine in short sessions, respects limited time (26)' }}
            number={1}
            compared={false}
            onCompare={vi.fn()}
            onOpen={vi.fn()}
            showLinux={false}
            busy={false}
            sifting={null}
            onAct={vi.fn()}
            onToo={vi.fn()}
          />
        </ToastProvider>
      </QueryClientProvider>,
    )
    expect(screen.getByText('Backlog players: fine in short sessions, respects limited time (26)')).toBeTruthy()
  })
})
