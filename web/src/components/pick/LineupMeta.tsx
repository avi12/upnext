import { Link } from 'react-router'
import type { PickBatch, PickResult } from '../../types'
import { durationText, plural } from '../../lib/format'
import { usageText } from '../../lib/jobs'
import { PLAN_WARNING, otherUse, pickShareText } from '../../lib/plan'
import { usePlan } from '../../lib/usePlan'
import { Money } from '../Money'

/** The small print under a lineup: how many games were considered, how
 *  long it took, the model, its tokens and cost, and its share of a plan. */
export function LineupMeta({ batch }: { batch: PickBatch }) {
  const meta = 'duration_ms' in batch ? (batch as PickResult) : null
  if (batch.mode === 'random' && meta) {
    return <p className="mt-4 font-mono text-[10.5px] text-dim">drawn from {plural(meta.candidates_considered ?? 0, 'game')} off your radar · no AI call</p>
  }
  if (batch.ranked_by && meta) {
    return <p className="mt-4 font-mono text-[10.5px] text-dim">{plural(meta.candidates_considered ?? 0, 'game')} ranked · no AI call</p>
  }
  if (!meta && !batch.usage) return null
  return (
    <p className="mt-4 font-mono text-[10.5px] text-dim">
      {[
        meta && `${plural(meta.candidates_considered ?? 0, 'game')} considered`,
        meta && durationText(meta.duration_ms),
        // A turned-down first answer doubles the tokens, so say so.
        meta && meta.attempts > 1 && `${meta.attempts} tries`,
        batch.model,
        batch.usage && usageText(batch.usage),
      ]
        .filter(Boolean)
        .join(' · ')}
      <Money cost={batch} link prefix=" · " />
      <PlanShare batch={batch} />
    </p>
  )
}

/** On a subscription, what the pick took of the plan once that's been measured, with what UpNext can't know on hover. */
function PlanShare({ batch }: { batch: PickBatch }) {
  const plan = usePlan()
  const p = plan.data
  if (!p?.subscription || !batch.usage) return null
  const share = pickShareText(p, batch)
  const title = p.measured
    ? `From ${p.measured.spans} ${p.measured.spans === 1 ? 'measurement' : 'measurements'} of your plan's usage bars; your other ${otherUse(p.backend)} use can make it look higher. ${PLAN_WARNING}`
    : `UpNext learns this from its first few picks or jobs. ${PLAN_WARNING}`
  return (
    <>
      {' · '}
      <Link to="/settings#ai-usage" title={title} className="underline decoration-underline underline-offset-2 hover:text-ink">
        {share ?? 'share of your plan not measured yet'}
      </Link>
    </>
  )
}
