import { useEffect, useId, useRef, useState, type KeyboardEvent } from 'react'
import { keepPreviousData, useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { AccountUsage, AiKind, AiTotals, DayUsage } from '../../types'
import { cx, relativeTime } from '../../lib/format'
import { tokensText, tookText, usageText } from '../../lib/jobs'
import { aiName, BACKEND_LABEL, KIND_LABEL } from '../../lib/ai'
import { costText, pricesSource, setShowUsd, useShowUsd } from '../../lib/money'
import { Money } from '../Money'
import { useAiPrices } from '../../lib/useAiPrices'
import { SentenceSelect } from '../library/SentenceSelect'
import { Checkbox, Ext, Section, TextButton } from './parts'
import { PlanUsage } from './PlanUsage'
import { possessive, usePlayers } from '../../lib/players'

/** What each kind counts: a title of summaries, a series of guides. */
const ITEM: Partial<Record<AiKind, [string, string]>> = {
  review_summaries: ['title', 'titles'],
  series_guides: ['series', 'series'],
  second_chances: ['game', 'games'],
  hidden_gems: ['game', 'games'],
}

const PERIODS = [7, 30, 90]

/** The order kinds are listed in, as the server orders its totals. */
const KIND_ORDER = Object.keys(KIND_LABEL) as AiKind[]

const TH = 'py-2.5 pr-4 text-[12.5px] font-normal text-dim'
const TD = 'py-2.5 pr-4 align-top tabular-nums'

function errText(e: unknown) {
  return e instanceof ApiError ? e.message : String(e)
}

function total(t: AiTotals) {
  return t.input_tokens + t.output_tokens
}

function callsText(t: AiTotals) {
  const calls = `${t.calls.toLocaleString()} ${t.calls === 1 ? 'call' : 'calls'}`
  return t.failed > 0 ? `${calls}, ${t.failed.toLocaleString()} failed` : calls
}

/** "Wed, Sep 30" from "2026-09-30", read as a local day. */
function dayText(day: string, style: 'short' | 'long' = 'long') {
  const [y, m, d] = day.split('-').map(Number)
  const date = new Date(y, m - 1, d)
  return date.toLocaleDateString(undefined, style === 'long' ? { weekday: 'short', month: 'short', day: 'numeric' } : { month: 'short', day: 'numeric' })
}

/** One headline figure: the tokens, then what they were. */
function Figure({ label, t }: { label: string; t: AiTotals }) {
  return (
    <div className="min-w-0">
      <div className="text-[13.5px] text-dim">{label}</div>
      <div className="mt-0.5 text-[26px] leading-tight font-semibold">{t.calls === 0 ? 'None' : `${tokensText(total(t))} tokens`}</div>
      {t.calls > 0 && (
        <div className="mt-0.5 text-[13.5px] text-muted">
          {tokensText(t.input_tokens)} in, {tokensText(t.output_tokens)} out · {callsText(t)}
          <Money cost={t} prefix=" · " />
        </div>
      )}
    </div>
  )
}

/** "Show costs in US dollars", and with it on, where the prices come from. */
function Dollars() {
  const show = useShowUsd()
  const prices = useAiPrices()
  const client = useQueryClient()
  const refresh = useMutation({
    mutationFn: api.aiPricesRefresh,
    onSuccess: (v) => {
      client.setQueryData(['aiPrices'], v)
      void client.invalidateQueries({ queryKey: ['aiUsage'] })
    },
  })
  const v = refresh.data ?? prices.data
  const p = v?.prices
  return (
    <div className="border-b border-line py-3">
      <Checkbox checked={show} onChange={setShowUsd} note={show ? undefined : 'Off, everything is in tokens, which mean the same everywhere.'}>
        Show costs in US dollars
      </Checkbox>
      {show && (
        <div className="mt-1 space-y-1 text-[13.5px] text-muted">
          {prices.isError && <p className="m-0 text-bad">Couldn't load the prices: {prices.error instanceof ApiError ? prices.error.message : String(prices.error)}</p>}
          {v && !p && <p className="m-0">UpNext can't read prices for the AI you use, so it shows no dollars for it. A model on this computer costs nothing per call.</p>}
          {p && (
            <>
              <p className="m-0">
                At {pricesSource(p)}. They may be out of date: <Ext href={p.page_url}>{p.label}'s pricing page</Ext> has the current ones.
                <TextButton className="ml-2 h-auto py-0" onClick={() => refresh.mutate()} disabled={refresh.isPending}>
                  {refresh.isPending ? 'Reading…' : 'Read them again'}
                </TextButton>
              </p>
              {p.error && (
                <p className="m-0 text-warn">
                  The last read{p.tried_at ? `, ${relativeTime(p.tried_at)},` : ''} failed: {p.error}. The prices above are still in use.
                </p>
              )}
              {!v.charged && (
                <p className="m-0">
                  Your {v.backend === 'codex-cli' ? 'ChatGPT' : 'Claude'} plan isn't charged per call: the dollars are what the same calls would cost
                  on the API, to compare, not money spent.
                </p>
              )}
            </>
          )}
        </div>
      )}
    </div>
  )
}

/** Clean steps for the token axis: 0, 50k, 100k … */
function niceTicks(max: number): number[] {
  if (max <= 0) return [0]
  const raw = max / 3
  const pow = 10 ** Math.floor(Math.log10(raw))
  const step = [1, 2, 2.5, 5, 10].map((f) => f * pow).find((s) => s >= raw) ?? 10 * pow
  const ticks: number[] = []
  for (let v = 0; v <= max + step * 0.001; v += step) ticks.push(v)
  if (ticks[ticks.length - 1] < max) ticks.push(ticks[ticks.length - 1] + step)
  return ticks
}

/** A column shape: 4px rounded at the top, square at the baseline. */
function columnPath(x: number, y: number, w: number, h: number) {
  const r = Math.min(4, w / 2, h)
  return `M${x},${y + h}V${y + r}Q${x},${y} ${x + r},${y}H${x + w - r}Q${x + w},${y} ${x + w},${y + r}V${y + h}Z`
}

const PLOT_H = 150
const AXIS_W = 44
const AXIS_H = 22

/** Tokens a day as columns, one series in the accent colour, with a tooltip per day by hover or arrow keys. */
function DayChart({ days, loading }: { days: DayUsage[]; loading: boolean }) {
  const wrapRef = useRef<HTMLDivElement>(null)
  const [width, setWidth] = useState(0)
  const [active, setActive] = useState<number | null>(null)
  const descId = useId()

  useEffect(() => {
    const el = wrapRef.current
    if (!el) return
    const ro = new ResizeObserver(([entry]) => setWidth(entry.contentRect.width))
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  const max = Math.max(...days.map(total), 0)
  const ticks = niceTicks(max)
  const top = ticks[ticks.length - 1] || 1
  const plotW = Math.max(0, width - AXIS_W)
  const slot = days.length ? plotW / days.length : 0
  const barW = Math.max(1, Math.min(24, slot - 2))
  const y = (v: number) => PLOT_H - (v / top) * PLOT_H
  // Label every day up to 10 days; otherwise about 5 dates and today.
  const every = days.length <= 10 ? 1 : Math.ceil(days.length / 5)
  const labelled = (i: number) => i === days.length - 1 || ((days.length - 1 - i) % every === 0 && days.length - 1 - i >= every * 0.6)

  const onKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const last = days.length - 1
    const at = active ?? last
    const next = e.key === 'ArrowLeft' ? at - 1 : e.key === 'ArrowRight' ? at + 1 : e.key === 'Home' ? 0 : e.key === 'End' ? last : null
    if (next == null) {
      if (e.key === 'Escape') setActive(null)
      return
    }
    e.preventDefault()
    setActive(Math.max(0, Math.min(last, next)))
  }

  const d = active != null ? days[active] : null
  // Beside the day's column, on whichever side has room, at the top of the plot.
  const tipRight = active != null && AXIS_W + (active + 0.5) * slot > width * 0.55
  const tipX = active != null ? AXIS_W + (tipRight ? active * slot - 8 : (active + 1) * slot + 8) : 0

  return (
    <div className={cx('relative mt-3 transition-opacity', loading && 'opacity-60')}>
      <p id={descId} className="sr-only">
        Tokens each day, oldest first. Use the left and right arrow keys to read a day; the table below has every value.
      </p>
      <div
        ref={wrapRef}
        tabIndex={0}
        role="group"
        aria-label="Tokens a day"
        aria-describedby={descId}
        onKeyDown={onKey}
        onFocus={() => setActive((a) => a ?? days.length - 1)}
        onBlur={() => setActive(null)}
        onPointerLeave={() => setActive(null)}
        className="rounded-md outline-offset-4"
        style={{ height: PLOT_H + AXIS_H }}
      >
        {width > 0 && (
          <svg width={width} height={PLOT_H + AXIS_H} aria-hidden="true" className="block overflow-visible">
            {ticks.map((t) => (
              <g key={t}>
                <line x1={AXIS_W} x2={width} y1={y(t) + 0.5} y2={y(t) + 0.5} stroke="var(--line)" strokeWidth={1} />
                <text x={AXIS_W - 8} y={y(t)} dy="0.32em" textAnchor="end" className="fill-[var(--dim)] font-mono text-[10px] tabular-nums">
                  {tokensText(t).replace('.0', '')}
                </text>
              </g>
            ))}
            {days.map((day, i) => {
              const v = total(day)
              const x = AXIS_W + i * slot + (slot - barW) / 2
              const h = PLOT_H - y(v)
              return (
                <g key={day.day}>
                  {active === i && <rect x={AXIS_W + i * slot} y={0} width={slot} height={PLOT_H} fill="var(--raised)" />}
                  {v > 0 && <path d={columnPath(x, y(v), barW, Math.max(h, 1.5))} fill="var(--accent)" />}
                  {labelled(i) && (
                    <text x={AXIS_W + i * slot + slot / 2} y={PLOT_H + 15} textAnchor="middle" className="fill-[var(--dim)] font-mono text-[10px]">
                      {i === days.length - 1 ? 'Today' : dayText(day.day, 'short')}
                    </text>
                  )}
                  {/* The whole day's column is the hover target, not just the bar. */}
                  <rect x={AXIS_W + i * slot} y={0} width={slot} height={PLOT_H + AXIS_H} fill="transparent" onPointerEnter={() => setActive(i)} />
                </g>
              )
            })}
          </svg>
        )}
      </div>
      {d && (
        <div
          role="status"
          className={cx(
            'pointer-events-none absolute top-0 z-10 w-max max-w-[260px] rounded-lg border border-line-strong bg-sheet px-3 py-2 text-[13px] shadow-xl',
            tipRight && '-translate-x-full',
          )}
          style={{ left: tipX }}
        >
          <div className="font-semibold text-ink">{d.calls === 0 ? 'No calls' : `${tokensText(total(d))} tokens`}</div>
          <div className="text-dim">
            {dayText(d.day)}
            {d.calls > 0 && ` · ${callsText(d)}`}
          </div>
          {d.calls > 0 && (
            <div className="text-muted">
              {usageText(d)}
              <Money cost={d} prefix=" · " />
            </div>
          )}
          {d.by_kind.length > 0 && (
            <ul className="m-0 mt-1 list-none space-y-0.5 p-0">
              {[...d.by_kind].sort((a, b) => KIND_ORDER.indexOf(a.kind) - KIND_ORDER.indexOf(b.kind)).map((k) => (
                <li key={k.kind} className="flex justify-between gap-4 text-muted">
                  <span>{KIND_LABEL[k.kind]}</span>
                  <span className="text-ink tabular-nums">{tokensText(total(k))}</span>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </div>
  )
}

/** Settings › AI usage: what the AI used, in tokens, by day, kind and model. */
export function AiUsageSection() {
  const [days, setDays] = useState(30)
  const [asTable, setAsTable] = useState(false)
  const show = useShowUsd()
  const usage = useQuery({ queryKey: ['aiUsage', days], queryFn: () => api.aiUsage(days), placeholderData: keepPreviousData })
  const u = usage.data
  const { multi, current } = usePlayers()
  const withCalls = u?.by_day.filter((d) => d.calls > 0) ?? []

  return (
    <Section id="ai-usage" title="AI usage" desc="What the AI used for picks, summaries and guides, in tokens, as the AI reports them.">
      {usage.isError && <p className="m-0 border-b border-line py-3 text-sm text-bad">Couldn't load it: {errText(usage.error)}</p>}
      {u && (
        <>
          <div className="grid grid-cols-1 gap-x-10 gap-y-4 border-b border-line py-4 sm:grid-cols-3">
            <Figure label="Today" t={u.today} />
            <Figure label="Last 7 days" t={u.last_7_days} />
            <Figure label={`Last ${u.days} days`} t={u.total} />
          </div>

          <PlanUsage />
          <Dollars />

          <div className="flex flex-wrap items-baseline justify-between gap-x-6 border-b border-line py-3">
            <p className="m-0 text-[15px] text-muted">
              By day, what for and which model, over the last{' '}
              <SentenceSelect
                label="Period"
                display={`${days} days`}
                value={String(days)}
                options={PERIODS.map((n) => ({ value: String(n), label: `${n} days` }))}
                onChange={(v) => setDays(Number(v))}
              />
            </p>
            {u.total.calls > 0 && (
              <TextButton aria-expanded={asTable} onClick={() => setAsTable((t) => !t)}>
                {asTable ? 'Show as a chart' : 'Show as a table'}
              </TextButton>
            )}
          </div>

          {u.total.calls === 0 ? (
            <p className="m-0 border-b border-line py-3 text-sm text-dim">No AI calls in the last {u.days} days.</p>
          ) : (
            <>
              {asTable ? (
                <div className="overflow-x-auto border-b border-line">
                  <table className="w-full min-w-[520px] border-collapse text-left text-sm">
                    <caption className="sr-only">Tokens a day, days with calls</caption>
                    <thead>
                      <tr className="border-b border-line">
                        <th scope="col" className={TH}>Day</th>
                        <th scope="col" className={TH}>Calls</th>
                        <th scope="col" className={TH}>Tokens in</th>
                        <th scope="col" className={TH}>Tokens out</th>
                        {show && <th scope="col" className={TH}>Cost</th>}
                        <th scope="col" className={TH}>What for</th>
                      </tr>
                    </thead>
                    <tbody>
                      {[...withCalls].reverse().map((d) => (
                        <tr key={d.day} className="border-b border-line-soft last:border-b-0">
                          <th scope="row" className={cx(TD, 'font-normal whitespace-nowrap')}>{dayText(d.day)}</th>
                          <td className={TD}>{callsText(d)}</td>
                          <td className={TD}>{tokensText(d.input_tokens)}</td>
                          <td className={TD}>{tokensText(d.output_tokens)}</td>
                          {show && <td className={TD}>{costText(d) ?? '–'}</td>}
                          <td className={cx(TD, 'text-muted')}>{d.by_kind.map((k) => KIND_LABEL[k.kind]).join(', ')}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                  <p className="m-0 pb-3 text-[12.5px] text-dim">Days without calls are left out.</p>
                </div>
              ) : (
                <div className="border-b border-line pb-3">
                  <DayChart days={u.by_day} loading={usage.isPlaceholderData} />
                </div>
              )}

              <div className="grid grid-cols-1 gap-x-14 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
                <div className="overflow-x-auto">
                  <table className="w-full min-w-[480px] border-collapse text-left text-sm">
                    <caption className="sr-only">By what it was for</caption>
                    <thead>
                      <tr className="border-b border-line">
                        <th scope="col" className={TH}>What for</th>
                        <th scope="col" className={TH}>Calls</th>
                        <th scope="col" className={TH}>Tokens in</th>
                        <th scope="col" className={TH}>Tokens out</th>
                        {show && <th scope="col" className={TH}>Cost</th>}
                        <th scope="col" className={TH}>Took</th>
                      </tr>
                    </thead>
                    <tbody>
                      {u.by_kind.map((k) => {
                        const item = ITEM[k.kind]
                        return (
                          <tr key={k.kind} className="border-b border-line">
                            <th scope="row" className={cx(TD, 'font-semibold')}>
                              {KIND_LABEL[k.kind]}
                              {item && k.items > 0 && (
                                <span className="block text-[12.5px] font-normal text-dim">
                                  {k.items.toLocaleString()} {k.items === 1 ? item[0] : item[1]}
                                </span>
                              )}
                            </th>
                            <td className={TD}>{callsText(k)}</td>
                            <td className={TD}>{tokensText(k.input_tokens)}</td>
                            <td className={TD}>{tokensText(k.output_tokens)}</td>
                            {show && <td className={TD}>{costText(k) ?? '–'}</td>}
                            <td className={cx(TD, 'whitespace-nowrap text-muted')}>{tookText(k.duration_ms / 1000)}</td>
                          </tr>
                        )
                      })}
                    </tbody>
                  </table>
                </div>
                <div className="overflow-x-auto">
                  <table className="w-full min-w-[320px] border-collapse text-left text-sm">
                    <caption className="sr-only">By model</caption>
                    <thead>
                      <tr className="border-b border-line">
                        <th scope="col" className={TH}>Model</th>
                        <th scope="col" className={TH}>Calls</th>
                        <th scope="col" className={TH}>Tokens</th>
                        {show && <th scope="col" className={TH}>Cost</th>}
                      </tr>
                    </thead>
                    <tbody>
                      {u.by_model.map((m) => (
                        <tr key={`${m.backend} ${m.model}`} className="border-b border-line">
                          <th scope="row" className={cx(TD, 'font-mono text-[12px] font-normal break-all')}>
                            {m.model ?? 'unknown'}
                            {m.backend !== u.backend && <span className="block font-sans text-[12.5px] text-dim">{BACKEND_LABEL[m.backend] ?? m.backend}</span>}
                          </th>
                          <td className={TD}>{m.calls.toLocaleString()}</td>
                          <td className={cx(TD, 'whitespace-nowrap')}>{usageText(m)}</td>
                          {show && <td className={TD}>{costText(m) ?? '–'}</td>}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              </div>
              {multi && (u.by_player?.length ?? 0) > 0 && (
                <div className="overflow-x-auto pt-2">
                  <table className="w-full min-w-[320px] border-collapse text-left text-sm">
                    <caption className="py-2 text-left text-[13.5px] text-dim">
                      By who asked: your own requests, and everyone’s on the AI accounts you own.
                    </caption>
                    <thead>
                      <tr className="border-b border-line">
                        <th scope="col" className={TH}>Who</th>
                        <th scope="col" className={TH}>Calls</th>
                        <th scope="col" className={TH}>Tokens</th>
                        {show && <th scope="col" className={TH}>Cost</th>}
                      </tr>
                    </thead>
                    <tbody>
                      {u.by_player?.map((p) => (
                        <tr key={p.player_id ?? 'library'} className="border-b border-line">
                          <th scope="row" className={cx(TD, 'font-semibold')}>
                            {p.name}
                            {p.player_id === current?.id && <span className="font-normal text-dim"> (you)</span>}
                          </th>
                          <td className={TD}>{callsText(p)}</td>
                          <td className={cx(TD, 'whitespace-nowrap')}>{usageText(p)}</td>
                          {show && <td className={TD}>{costText(p) ?? '–'}</td>}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              {(multi || (u.by_account?.length ?? 0) > 1) && (u.by_account?.length ?? 0) > 0 && (
                <div className="overflow-x-auto pt-2">
                  <table className="w-full min-w-[320px] border-collapse text-left text-sm">
                    <caption className="py-2 text-left text-[13.5px] text-dim">
                      By AI account: every call on the ones you own, and your own on the ones shared with you.
                    </caption>
                    <thead>
                      <tr className="border-b border-line">
                        <th scope="col" className={TH}>AI account</th>
                        <th scope="col" className={TH}>Calls</th>
                        <th scope="col" className={TH}>Tokens</th>
                        {show && <th scope="col" className={TH}>Cost</th>}
                      </tr>
                    </thead>
                    <tbody>
                      {u.by_account?.map((a) => (
                        <tr key={a.ai_account_id ?? 'none'} className="border-b border-line">
                          <th scope="row" className={cx(TD, 'font-semibold')}>
                            {accountName(a)}
                            <span className="block text-[12.5px] font-normal text-dim">{whoseAccount(a)}</span>
                          </th>
                          <td className={TD}>{callsText(a)}</td>
                          <td className={cx(TD, 'whitespace-nowrap')}>{usageText(a)}</td>
                          {show && <td className={TD}>{costText(a) ?? '–'}</td>}
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
            </>
          )}
        </>
      )}
    </Section>
  )
}

/** An AI account's row in Usage: what it runs on. */
function accountName(a: AccountUsage): string {
  if (a.ai_account_id != null && a.owner == null) return 'A removed AI account'
  return aiName(a)
}

/** Whose an AI account is, beside its name in Usage. */
function whoseAccount(a: AccountUsage): string {
  if (a.mine) return 'Yours'
  if (a.owner) return `${possessive(a.owner)}, shared with you`
  return 'No longer here'
}
