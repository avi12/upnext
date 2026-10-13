import { ApiError } from '../../api'
import type { AiKind, PlanShare, PlanSummary } from '../../types'
import { cx, relativeTime } from '../../lib/format'
import { PLAN_WARNING, otherUse, pctText, planTitle, resetsText, windowName, windowNoun } from '../../lib/plan'
import { Bar } from '../ProgressBar'
import { usePlan } from '../../lib/usePlan'
import { TextButton } from './parts'

/** What one of each kind is, in a row label. */
const EACH: Record<AiKind, string> = {
  pick: 'A pick',
  decide: 'A Help me decide',
  too: 'An "It\'s too ___"',
  review_summaries: "One title's People say, Vibes and tags",
  series_guides: 'A series guide',
  second_chances: "One game's second chance",
  hidden_gems: "One game's hidden-gem rating",
  test: 'A test',
}

const TH = 'py-2 pr-4 text-[12.5px] font-normal text-dim'
const TD = 'py-2 pr-4 align-top tabular-nums'

function ShareRow({ label, share, ids }: { label: string; share: PlanShare; ids: string[] }) {
  return (
    <tr className="border-b border-line-soft last:border-b-0">
      <th scope="row" className={cx(TD, 'font-normal')}>
        {label}
      </th>
      {ids.map((id) => (
        <td key={id} className={TD}>
          {share.percent[id] != null ? pctText(share.percent[id]) : '–'}
        </td>
      ))}
    </tr>
  )
}

/** What UpNext has measured its AI takes of the plan, or that it hasn't yet. */
function Measured({ plan }: { plan: PlanSummary }) {
  const m = plan.measured
  if (!m) {
    return (
      <p className="m-0 mt-3 text-sm text-muted">
        UpNext will learn what a pick or a run of summaries takes of your plan from its first few picks or jobs. It reads these bars around each
        call, which costs nothing.
      </p>
    )
  }
  const ids = m.windows.map((w) => w.id)
  const derived = m.windows.filter((w) => w.from_session != null)
  return (
    <div className="mt-3">
      <div className="overflow-x-auto">
        <table className="w-full min-w-[420px] border-collapse text-left text-sm">
          <caption className="sr-only">What UpNext's AI takes of your plan</caption>
          <thead>
            <tr className="border-b border-line">
              <th scope="col" className={TH}>
                Takes of your plan
              </th>
              {m.windows.map((w) => (
                <th key={w.id} scope="col" className={TH}>
                  {windowName(w.id, w.label, plan.backend)}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {m.each
              .filter((s) => s.kind !== 'test')
              .map((s) => (
                <ShareRow key={s.kind} label={EACH[s.kind]} share={s} ids={ids} />
              ))}
            {m.one_time && (
              <ShareRow
                label={`People say, Vibes and tags for the ${m.one_time.items.toLocaleString()} ${m.one_time.items === 1 ? 'title' : 'titles'} still without them`}
                share={m.one_time}
                ids={ids}
              />
            )}
          </tbody>
        </table>
      </div>
      <p className="m-0 mt-2 text-[12.5px] text-dim">
        From {m.spans.toLocaleString()} {m.spans === 1 ? 'measurement' : 'measurements'}: what UpNext's calls used from one tick of a bar to the
        next, {m.measure === 'api_usd' ? 'with tokens weighed by their API price' : 'in tokens in and out'}. Your other {otherUse(plan.backend)} use can
        make these look higher, never lower.
        {derived.length > 0 &&
          ` ${derived.map((w) => windowName(w.id, w.label, plan.backend)).join(' and ')} is worked out from the ${windowNoun('session', plan.backend).slice(4)}'s figure, by how far both bars moved together.`}
      </p>
    </div>
  )
}

/** Settings › AI usage, on a subscription: the plan's bars as the CLI reports them, what UpNext's AI takes of them, and what it can't know. */
export function PlanUsage() {
  const plan = usePlan()
  const p = plan.data
  if (!p?.subscription) return null
  const reading = p.reading
  return (
    <div className="border-b border-line py-4">
      <div className="flex flex-wrap items-baseline justify-between gap-x-6">
        <h3 className="m-0 text-[15px] font-semibold">
          {planTitle(p)}
        </h3>
        <span className="text-[13px] text-dim">
          {reading && `Read ${relativeTime(reading.at)}`}
          <TextButton className="ml-3" onClick={() => void plan.refetch()} disabled={plan.isFetching}>
            {plan.isFetching ? 'Reading…' : 'Read again'}
          </TextButton>
        </span>
      </div>
      {plan.isError && <p className="m-0 text-sm text-bad">Couldn't load it: {plan.error instanceof ApiError ? plan.error.message : String(plan.error)}</p>}
      {p.error && <p className="m-0 mt-1 text-sm text-warn">{p.error[0].toUpperCase() + p.error.slice(1)}.</p>}
      {reading && (
        <ul className="m-0 mt-2 grid list-none gap-3 p-0">
          {reading.windows.map((w) => {
            const name = windowName(w.id, w.label, p.backend)
            const resets = resetsText(w)
            return (
              <li key={w.id} className="grid grid-cols-1 items-center gap-x-5 gap-y-1 sm:grid-cols-[150px_minmax(0,1fr)_minmax(0,auto)]">
                <span className="text-sm">{name}</span>
                <Bar done={w.used} total={100} label={`${name}: ${Math.round(w.used)}% used`} tone={w.used >= 80 ? 'warn' : 'accent'} />
                <span className="text-[13px] text-muted tabular-nums">
                  {Math.round(w.used)}% used{resets && ` · resets ${resets}`}
                </span>
              </li>
            )
          })}
        </ul>
      )}
      {reading && reading.windows.length === 0 && <p className="m-0 mt-2 text-sm text-muted">Codex reports no usage limits for this plan.</p>}
      <Measured plan={p} />
      <p className="m-0 mt-3 text-sm text-muted">
        {PLAN_WARNING}{' '}
        {p.backend === 'codex-cli'
          ? "The bars are your plan's Codex limits, shared with Codex wherever you use it, on any device."
          : "The bars are your whole plan's, including claude.ai and Claude Code on any device."}
      </p>
    </div>
  )
}
