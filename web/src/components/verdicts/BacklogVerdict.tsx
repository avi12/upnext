import { useState } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { GameView, VerdictAnswers, VerdictConsent, VerdictForm, VerdictProblem } from '../../types'
import { cx } from '../../lib/format'
import { SOURCE_WORDS, fieldShows, shownAnswers, stateText, textRefusal } from '../../lib/verdicts'
import { Section } from '../library/SheetSections'
import { CommunityVerdicts } from './CommunityVerdicts'
import { keys, useSettings } from '../../lib/queries'

/** The game page's "Backlog verdicts": what other players say
 *  (`CommunityVerdicts`), then your own verdict about the game, with Write,
 *  Edit, Update or Take down, and after a mark that ends something, one
 *  line offering it. Never a pop-up: the form and the
 *  consent screen open in place. */
export function BacklogVerdict({ game, offer, onOfferDone }: { game: GameView; offer?: boolean; onOfferDone?: () => void }) {
  const qc = useQueryClient()
  const form = useQuery({ queryKey: ['verdict', game.id], queryFn: () => api.verdictForm(game.id), staleTime: 30_000 })
  const settings = useSettings({ enabled: !!offer })
  const [open, setOpen] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const refresh = () => {
    qc.invalidateQueries({ queryKey: ['verdict', game.id] })
    qc.invalidateQueries({ queryKey: ['verdicts'] })
  }
  const takeDown = useMutation({
    mutationFn: () => api.withdrawVerdict(game.id),
    onSuccess: (r) => {
      setNote(r.verdict ? "Taken down: it's left out of the next daily update and gone from the service within a week." : 'Draft deleted.')
      refresh()
    },
    onError: (e) => setNote(e instanceof Error ? e.message : String(e)),
  })
  const dontAsk = useMutation({
    mutationFn: () => api.updateSettings({ verdicts_ask: false }),
    onSuccess: (s) => {
      qc.setQueryData(keys.settings, s)
      onOfferDone?.()
    },
  })

  const f = form.data
  if (!f) return null
  const mine = f.mine
  const out = mine && (mine.state === 'sent' || mine.state === 'held')
  const live = mine && mine.state !== 'withdrawn' && mine.state !== 'removed'
  const showOffer = offer && !open && !out && !f.cant_send && settings.data?.verdicts_ask !== false

  return (
    <Section title="Backlog verdicts" id={`verdicts-${game.id}`}>
      <CommunityVerdicts game={game} />
      {f.cant_send ? (
        <p className="m-0 text-[14px] text-dim">{f.cant_send}</p>
      ) : open ? (
        <VerdictEditor
          form={f}
          onClose={(message) => {
            setOpen(false)
            setNote(message ?? null)
            refresh()
            onOfferDone?.()
          }}
        />
      ) : (
        <>
          {showOffer && (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg border border-line bg-raised px-3 py-2" role="group" aria-label="Offer a verdict">
              <span className="text-[14px] text-ink">Tell other backlog players how it went?</span>
              <button type="button" className="btn-quiet h-9 px-2 text-[14px] text-accent" onClick={() => setOpen(true)}>
                Write a verdict
              </button>
              <button type="button" className="btn-quiet h-9 px-2 text-[13.5px] text-dim" onClick={() => onOfferDone?.()}>
                Not now
              </button>
              <button type="button" className="btn-quiet h-9 px-2 text-[13.5px] text-dim" onClick={() => dontAsk.mutate()}>
                Don't ask again
              </button>
            </div>
          )}
          {mine && live ? (
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
              <span className="text-[14px] text-muted">
                Your verdict: <span className="text-ink">{stateText(mine)}</span>
              </span>
              <button type="button" className="btn-quiet h-9 px-1 text-[14px] underline decoration-underline underline-offset-4" onClick={() => setOpen(true)}>
                {f.outdated ? 'Update your verdict' : out ? 'Edit' : 'Edit and send'}
              </button>
              <button
                type="button"
                className="btn-quiet h-9 px-1 text-[14px] text-dim underline decoration-underline underline-offset-4"
                disabled={takeDown.isPending}
                onClick={() => takeDown.mutate()}
              >
                {mine.remote_id ? 'Take down' : 'Delete draft'}
              </button>
            </div>
          ) : (
            !showOffer && (
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1">
                {mine && <span className="text-[14px] text-dim">{stateText(mine)}.</span>}
                <button type="button" className="btn-outline h-9 px-3 text-[14px]" onClick={() => setOpen(true)}>
                  Write a backlog verdict
                </button>
              </div>
            )
          )}
          {mine?.error && live && <p className="m-0 text-[13px] text-dim">{mine.error}</p>}
        </>
      )}
      {note && !open && (
        <p className="m-0 text-[13.5px] text-muted" role="status">
          {note}
        </p>
      )}
    </Section>
  )
}

/** The form, and the consent screen before the first send. */
export function VerdictEditor({ form, onClose }: { form: VerdictForm; onClose: (message?: string) => void }) {
  const contract = useQuery({ queryKey: ['verdict-contract'], queryFn: api.verdictContract, staleTime: Infinity })
  const [answers, setAnswers] = useState<VerdictAnswers>(form.answers)
  const [problems, setProblems] = useState<VerdictProblem[]>([])
  const [error, setError] = useState<string | null>(null)
  const [consent, setConsent] = useState<VerdictConsent | null>(null)
  const [accepted, setAccepted] = useState(form.consent.current)
  const [busy, setBusy] = useState(false)
  const sent = form.mine?.state === 'sent' || form.mine?.state === 'held'

  const c = contract.data
  if (!c) return contract.isError ? <p className="m-0 text-[14px] text-dim">Couldn't load the questions.</p> : null

  const ready = shownAnswers(c.fields, answers)
  const typed = [
    textRefusal(c, ready.stop_note ?? '', c.limits.stop_note_chars),
    textRefusal(c, ready.text ?? '', c.limits.text_chars),
  ]
  const missing = !ready.outcome || !ready.worth

  const fail = (e: unknown) => {
    if (e instanceof ApiError) {
      setError(e.message)
      setProblems(e.problems)
    } else setError(String(e))
  }
  const send = async (agreed = accepted) => {
    if (!agreed) {
      setConsent(form.consent)
      return
    }
    setBusy(true)
    setError(null)
    try {
      const r = await api.sendVerdict(form.title_id, ready)
      onClose(r.message)
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }
  const save = async () => {
    setBusy(true)
    try {
      await api.saveVerdict(form.title_id, ready)
      onClose('Saved as a draft on this computer. Nothing was sent.')
    } catch (e) {
      fail(e)
    } finally {
      setBusy(false)
    }
  }

  if (consent) {
    return (
      <ConsentScreen
        consent={consent}
        busy={busy}
        onAccept={async () => {
          setBusy(true)
          try {
            await api.acceptVerdictConsent(consent.version)
          } catch (e) {
            setBusy(false)
            fail(e)
            setConsent(null)
            return
          }
          setAccepted(true)
          setConsent(null)
          await send(true)
        }}
        onDecline={() => setConsent(null)}
      />
    )
  }

  const set = (name: string, value: string | string[] | null) => setAnswers((a) => ({ ...a, [name]: value }))
  const problemFor = (name: string) => problems.find((p) => p.field === name)?.why

  return (
    <form
      className="flex flex-col gap-4 rounded-xl border border-line p-4"
      aria-label="Your backlog verdict"
      onSubmit={(e) => {
        e.preventDefault()
        void send()
      }}
    >
      {c.fields
        .filter((fld) => fieldShows(fld, answers))
        .map((fld) => {
          const from = form.from[fld.name]
          const why = problemFor(fld.name)
          const head = (
            <legend className="mb-1.5 p-0 text-[14px] font-semibold text-ink">
              {fld.question}
              {fld.required && <span className="font-normal text-dim"> (needed)</span>}
              {from && <span className="ml-2 text-[12.5px] font-normal text-dim">{SOURCE_WORDS[from]}</span>}
            </legend>
          )
          if (fld.text) {
            const value = (answers[fld.name as keyof VerdictAnswers] as string | null | undefined) ?? ''
            const refusal = fld.name === 'text' ? typed[1] : typed[0]
            const max = fld.max_chars ?? 500
            const Input = fld.name === 'text' ? 'textarea' : 'input'
            return (
              <fieldset key={fld.name} className="m-0 border-0 p-0">
                {head}
                <Input
                  value={value}
                  aria-label={fld.question}
                  onChange={(e) => set(fld.name, e.target.value)}
                  rows={fld.name === 'text' ? 3 : undefined}
                  className="w-full rounded-lg border border-line-strong bg-ground px-3 py-2 text-[14px] text-ink placeholder:text-dim"
                  placeholder={fld.name === 'stop_note' ? 'after chapter 3' : undefined}
                />
                <p className="m-0 mt-1 flex justify-between gap-3 text-[12.5px] text-dim">
                  <span className={cx((refusal || why) && 'text-bad')}>{refusal ?? why ?? (fld.name === 'text' ? form.consent.text_box_line : '')}</span>
                  <span className="font-mono">
                    {[...value].length}/{max}
                  </span>
                </p>
              </fieldset>
            )
          }
          const current = answers[fld.name as keyof VerdictAnswers]
          const list = fld.many ? ((current as string[] | undefined) ?? []) : []
          const max = fld.name === 'factors' ? c.limits.factors : Infinity
          const choices = fld.choices ?? []
          const groups = fld.name === 'factors' ? [['Helps', choices.filter((x) => x.helps)], ['Hurts', choices.filter((x) => !x.helps)]] as const : [[null, choices]] as const
          return (
            <fieldset key={fld.name} className="m-0 border-0 p-0">
              {head}
              {groups.map(([title, opts]) => (
                <div key={title ?? 'all'} className="mb-1 flex flex-wrap items-center gap-1.5">
                  {title && <span className="mr-1 font-mono text-[10.5px] tracking-[0.08em] text-dim uppercase">{title}</span>}
                  {opts.map((o) => {
                    const on = fld.many ? list.includes(o.code) : current === o.code
                    const full = fld.many && !on && list.length >= max
                    return (
                      <button
                        key={o.code}
                        type="button"
                        aria-pressed={on}
                        disabled={full}
                        onClick={() =>
                          fld.many ? set(fld.name, on ? list.filter((x) => x !== o.code) : [...list, o.code]) : set(fld.name, on ? null : o.code)
                        }
                        className={cx(
                          'min-h-8 rounded-full border px-3 text-left text-[13px] leading-tight disabled:opacity-40',
                          on ? 'border-accent bg-accent text-accent-ink' : 'border-line-strong text-ink hover:bg-raised',
                        )}
                      >
                        {o.label}
                      </button>
                    )
                  })}
                </div>
              ))}
              {why && <p className="m-0 text-[12.5px] text-bad">{why}</p>}
            </fieldset>
          )
        })}
      {!form.service && <p className="m-0 text-[13px] text-dim">No verdicts service is set on this install yet, so it can only be saved as a draft.</p>}
      {error && (
        <p className="m-0 text-[13.5px] text-bad" role="alert">
          {error}
        </p>
      )}
      <div className="flex flex-wrap items-center justify-end gap-2">
        <button type="button" className="btn-quiet h-10 px-3 text-[14px]" onClick={() => onClose()}>
          Cancel
        </button>
        {!sent && (
          <button type="button" className="btn-outline h-10 px-3 text-[14px]" disabled={busy} onClick={() => void save()}>
            Save draft
          </button>
        )}
        <button
          type="submit"
          className="btn-accent h-10 rounded-lg px-4 text-[14px] disabled:opacity-50"
          disabled={busy || missing || !!typed[0] || !!typed[1] || !form.service}
          title={missing ? 'Answer where you left it and whether it was worth it' : undefined}
        >
          {sent ? 'Send changes' : 'Send'}
        </button>
      </div>
    </form>
  )
}

/** "Your verdict will be public.": shown before the first verdict leaves the
 *  computer, and again when its words change. Only Publish sends anything. */
export function ConsentScreen({
  consent,
  busy,
  onAccept,
  onDecline,
}: {
  consent: VerdictConsent
  busy: boolean
  onAccept: () => void
  onDecline: () => void
}) {
  return (
    <div className="flex flex-col gap-3 rounded-xl border border-line-strong p-4" role="group" aria-labelledby="verdict-consent-title">
      <h4 id="verdict-consent-title" className="m-0 font-display text-[18px] font-bold">
        {consent.title}
      </h4>
      {consent.paragraphs.map((p) => (
        <p key={p} className="m-0 text-[14.5px] leading-[1.55] text-muted">
          {p}
        </p>
      ))}
      <p className="m-0 text-[13px] text-dim">
        <a href={consent.licence_url} target="_blank" rel="noreferrer" className="underline underline-offset-4">
          CC0 1.0
        </a>
        {consent.terms_url && (
          <>
            {' · '}
            <a href={consent.terms_url} target="_blank" rel="noreferrer" className="underline underline-offset-4">
              The service's terms
            </a>
          </>
        )}
      </p>
      <div className="flex flex-wrap items-center justify-end gap-2">
        <button type="button" className="btn-quiet h-10 px-3 text-[14px]" onClick={onDecline}>
          {consent.decline}
        </button>
        <button type="button" className="btn-accent h-10 rounded-lg px-4 text-[14px]" disabled={busy} onClick={onAccept}>
          {consent.accept}
        </button>
      </div>
    </div>
  )
}
