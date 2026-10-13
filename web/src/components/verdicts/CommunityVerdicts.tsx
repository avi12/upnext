import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { CommunityStatus, GameView, GameVerdicts, ShownTextView, VerdictConsent } from '../../types'
import { ConsentScreen } from './BacklogVerdict'
import { cx, monthYear } from '../../lib/format'
import { snapshotDay } from '../../lib/verdicts'
import { keys } from '../../lib/queries'

/** The texts shown before "More". */
const FIRST_TEXTS = 3

/** Each outcome's colour in "How far people got". */
const OUTCOME_COLOUR: Record<string, string> = {
  finished: 'bg-good',
  playing: 'bg-accent',
  endless: 'bg-accent/60',
  shelved: 'bg-warn',
  stopped: 'bg-bad',
  bounced: 'bg-line-strong',
}

/** What other backlog players say about a game, at the
 *  top of the game page's "Backlog verdicts": the headline, what most of
 *  them say, how far people got, the newest texts and where they came from.
 *  While they're off, nothing, except once on an install that never chose:
 *  a note offering them. */
export function CommunityVerdicts({ game }: { game: GameView }) {
  const page = useQuery({ queryKey: ['community-verdicts', game.id], queryFn: () => api.gameVerdicts(game.id), staleTime: 60_000 })
  const p = page.data
  if (!p) return null
  if (!p.on) return !p.chosen && p.service ? <TurnOnNote /> : null
  if (!p.verdicts) return null
  return <Summary v={p.verdicts} status={p} />
}

/** The one-time note for an install that never chose ("The
 *  switch"): it's a download that sends nothing about the player, so it's offered here once. */
function TurnOnNote() {
  const qc = useQueryClient()
  const [note, setNote] = useState<string | null>(null)
  const choose = useMutation({
    mutationFn: async (on: boolean) => {
      const s = await api.updateSettings({ verdicts_receive: on })
      qc.setQueryData(keys.settings, s)
      if (on) {
        try {
          await api.runTask('verdicts')
        } catch {
          return 'On. They download with the next daily run.'
        }
        return 'On. Downloading them now.'
      }
      return null
    },
    onSuccess: (message) => {
      setNote(message)
      qc.invalidateQueries({ queryKey: ['community-verdicts'] })
      qc.invalidateQueries({ queryKey: ['verdicts-community'] })
    },
    onError: (e) => setNote(e instanceof Error ? e.message : String(e)),
  })
  if (note) {
    return (
      <p className="m-0 text-[13.5px] text-muted" role="status">
        {note}
      </p>
    )
  }
  if (choose.isSuccess) return null
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-line bg-raised px-3 py-2" role="group" aria-label="Other players' verdicts">
      <span className="text-[14px] text-ink">
        See what other backlog players say about games? It's a daily download from the verdicts service that sends nothing about you.
      </span>
      <button type="button" className="btn-quiet h-9 px-2 text-[14px] text-accent" disabled={choose.isPending} onClick={() => choose.mutate(true)}>
        Turn them on
      </button>
      <button type="button" className="btn-quiet h-9 px-2 text-[13.5px] text-dim" disabled={choose.isPending} onClick={() => choose.mutate(false)}>
        No thanks
      </button>
    </div>
  )
}

function Summary({ v, status }: { v: GameVerdicts; status: CommunityStatus }) {
  const [all, setAll] = useState(false)
  const texts = all ? v.texts : v.texts.slice(0, FIRST_TEXTS)
  const total = v.outcomes.reduce((n, o) => n + o.count, 0)
  const day = status.snapshot.date
  return (
    <div className="flex flex-col gap-2.5" data-testid="community-verdicts">
      <p className="m-0 text-base font-semibold text-ink">{v.headline}</p>
      {v.lines.length > 0 && (
        <ul className="m-0 flex list-none flex-col gap-0.5 p-0 text-[14.5px] leading-[1.6]">
          {v.lines.map((l) => (
            <li key={l}>
              <span className="text-dim" aria-hidden="true">
                Most players:{' '}
              </span>
              <span className="sr-only">Most players say: </span>
              {l}
            </li>
          ))}
        </ul>
      )}
      {v.counts_line && <p className="m-0 text-[14.5px] text-muted">{v.counts_line}.</p>}
      {total > 0 && (
        <div className="flex flex-col gap-1.5">
          <span className="text-[13px] text-dim">How far people got</span>
          <div className="flex h-2 w-full max-w-md overflow-hidden rounded-full bg-raised" aria-hidden="true">
            {v.outcomes.map((o) => (
              <span key={o.outcome} className={cx('h-full', OUTCOME_COLOUR[o.outcome] ?? 'bg-line-strong')} style={{ width: `${(o.count / total) * 100}%` }} />
            ))}
          </div>
          <ul className="m-0 flex list-none flex-col gap-0.5 p-0 text-[14px] text-muted">
            {v.outcomes.map((o) => (
              <li key={o.outcome} className="flex items-center gap-2">
                <span className={cx('h-2 w-2 shrink-0 rounded-full', OUTCOME_COLOUR[o.outcome] ?? 'bg-line-strong')} aria-hidden="true" />
                <span>
                  {o.label} <span className="font-mono text-[12px] text-dim">({o.count})</span>
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
      {texts.length > 0 && (
        <ul className="m-0 flex list-none flex-col gap-3 p-0">
          {texts.map((t) => (
            <Text key={t.id} t={t} />
          ))}
        </ul>
      )}
      {v.texts.length > FIRST_TEXTS && (
        <button type="button" className="btn-quiet h-9 self-start px-0 text-[14px] underline decoration-underline underline-offset-4" onClick={() => setAll((a) => !a)}>
          {all ? 'Fewer' : `More (${v.texts.length - FIRST_TEXTS})`}
        </button>
      )}
      <p className="m-0 text-[13px] text-dim">
        From {v.n} UpNext {v.n === 1 ? 'player' : 'players'}
        {day ? `, updated ${snapshotDay(day)}` : ''}. Public under{' '}
        <a href="https://creativecommons.org/publicdomain/zero/1.0/" target="_blank" rel="noreferrer" className="underline underline-offset-4">
          CC0
        </a>
        .
      </p>
    </div>
  )
}

/** One player's text, with its answers, its month and Report. */
function Text({ t }: { t: ShownTextView }) {
  const contract = useQuery({ queryKey: ['verdict-contract'], queryFn: api.verdictContract, staleTime: Infinity })
  const [open, setOpen] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  // A report goes with the player's anonymous key, which the consent screen
  // explains, so before it's accepted the screen shows, as it does on send.
  const [consent, setConsent] = useState<{ screen: VerdictConsent; reason: string } | null>(null)
  const report = useMutation({
    mutationFn: (reason: string) => api.reportVerdict(t.id, reason),
    onSuccess: () => {
      setOpen(false)
      setNote('Reported. Thanks: the service hides a text that several players report.')
    },
    onError: async (e, reason) => {
      setOpen(false)
      if (e instanceof ApiError && e.status === 409) {
        const screen = await api.verdictConsent().catch(() => null)
        if (screen && !screen.current) {
          setConsent({ screen, reason })
          return
        }
      }
      setNote(e instanceof ApiError ? e.message : String(e))
    },
  })
  const accept = useMutation({
    mutationFn: (version: number) => api.acceptVerdictConsent(version),
    onSuccess: () => {
      const reason = consent?.reason
      setConsent(null)
      if (reason) report.mutate(reason)
    },
    onError: (e) => {
      setConsent(null)
      setNote(e instanceof ApiError ? e.message : String(e))
    },
  })
  const reasons = contract.data?.report_reasons ?? []
  if (consent) {
    return (
      <li className="flex flex-col gap-1">
        <ConsentScreen
          consent={consent.screen}
          busy={accept.isPending}
          onAccept={() => accept.mutate(consent.screen.version)}
          onDecline={() => setConsent(null)}
        />
      </li>
    )
  }
  return (
    <li className="flex flex-col gap-1 border-l-2 border-line pl-3">
      {t.text && <p className="m-0 text-[14.5px] leading-[1.6] text-ink">{t.text}</p>}
      {t.stop_note && <p className="m-0 text-[14px] text-muted">Fine to stop {t.stop_note}</p>}
      <p className="m-0 flex flex-wrap items-center gap-x-3 text-[12.5px] text-dim">
        <span>
          {t.about} · {monthYear(`${t.month}-01`)}
        </span>
        {!note && !open && (
          <button type="button" className="btn-quiet h-8 px-0 text-[12.5px] underline decoration-underline underline-offset-4" onClick={() => setOpen(true)}>
            Report
          </button>
        )}
      </p>
      {open && (
        <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Why report it?">
          {reasons.map((r) => (
            <button
              key={r.code}
              type="button"
              className="min-h-8 rounded-full border border-line-strong px-3 text-[13px] text-ink hover:bg-raised disabled:opacity-50"
              disabled={report.isPending}
              onClick={() => report.mutate(r.code)}
            >
              {r.label}
            </button>
          ))}
          <button type="button" className="btn-quiet h-8 px-2 text-[13px] text-dim" onClick={() => setOpen(false)}>
            Cancel
          </button>
        </div>
      )}
      {note && (
        <p className="m-0 text-[12.5px] text-muted" role="status">
          {note}
        </p>
      )}
    </li>
  )
}
