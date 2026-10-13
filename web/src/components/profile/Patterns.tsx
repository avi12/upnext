import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { TastePattern, TasteResponse } from '../../types'
import { relativeTime } from '../../lib/format'
import { dayText } from '../../lib/statuses'
import { SentenceChoice } from '../SentenceMenu'
import { Section, TextButton } from '../settings/parts'
import { andList, games, OUTCOME } from './words'

const MIN_GAMES = ['3', '4', '5', '6', '8', '10']
/** Two half-lives: how far back the games a pattern needs can be. */
function recentText(halfLifeDays: number): string {
  const days = halfLifeDays * 2
  const years = days / 365
  if (Math.abs(years - Math.round(years)) < 0.02 && Math.round(years) >= 1) {
    const n = Math.round(years)
    return n === 1 ? 'the last year' : `the last ${['', '', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten'][n] ?? n} years`
  }
  return `the last ${days} days`
}

const HALF_LIFE = [
  { value: '182', label: 'after six months' },
  { value: '365', label: 'after a year' },
  { value: '730', label: 'after two years' },
  { value: '1825', label: 'after five years' },
]

/** Patterns counted from what you finish and what you leave, each with its evidence. */
export function PatternsSection({
  data,
  updating,
  onUpdate,
  onAnswer,
  onOpen,
  onRule,
}: {
  data: TasteResponse
  /** The profile job is running. */
  updating: boolean
  onUpdate: () => void
  onAnswer: (p: TastePattern, answer: 'right' | 'not_really' | 'show') => void
  onOpen: (id: number) => void
  onRule: (patch: { pattern_min_games?: string; pattern_half_life_days?: string }) => void
}) {
  const [hiddenOpen, setHiddenOpen] = useState(false)
  const { judged, patterns, hidden_patterns: hidden, rules } = data
  const halfLife = String(rules.half_life_days)
  const halfLifeOptions = HALF_LIFE.some((h) => h.value === halfLife) ? HALF_LIFE : [...HALF_LIFE, { value: halfLife, label: `after ${halfLife} ${halfLife === '1' ? 'day' : 'days'}` }]
  const minOptions = (MIN_GAMES.includes(String(rules.min_games)) ? MIN_GAMES : [...MIN_GAMES, String(rules.min_games)]).map((n) => ({ value: n, label: n }))

  return (
    <Section id="patterns" title="Patterns" desc="Counted from the games you stick with and the ones you leave">
      <div className="mt-4 flex max-w-[860px] flex-col gap-1 text-[15px] leading-[1.6] text-muted">
        <p className="m-0">
          From {games(judged.kept)} you stuck with (favourites, staples, beaten, played through, picked and played) and {judged.left} you left
          (shelved, abandoned, not for you, turned down). A game you put down only because life got busy doesn’t count.
        </p>
        <p className="m-0">
          A pattern needs{' '}
          <SentenceChoice label="Games a pattern needs" value={String(rules.min_games)} options={minOptions} onChange={(v) => onRule({ pattern_min_games: v })} />{' '}
          games going its way from {recentText(rules.half_life_days)}, and a game counts half{' '}
          <SentenceChoice label="When evidence counts half" value={halfLife} options={halfLifeOptions} onChange={(v) => onRule({ pattern_half_life_days: v })} />
          , since tastes change. Older games still count, a little. Favourites never fade.
        </p>
        <p className="m-0 flex flex-wrap items-center gap-x-3">
          <span>
            {updating ? 'Counting now…' : data.updated_at ? `Counted ${relativeTime(data.updated_at)}, and after every sync.` : 'Not counted yet; it runs after every sync.'}
          </span>
          <TextButton onClick={onUpdate} disabled={updating}>
            Count again now
          </TextButton>
        </p>
      </div>

      {patterns.length === 0 ? (
        <p className="m-0 mt-5 max-w-[760px] text-[15.5px] leading-[1.6] text-ink">
          {judged.kept === 0 || judged.left === 0
            ? 'Nothing to compare yet: every game that counts went the same way. Mark what you’ve beaten, or say why you stopped a game, and patterns can show.'
            : `No patterns yet. None of what your games are like splits clearly enough between the ones you stick with and the ones you leave, with ${rules.min_games} games or more behind it.`}
        </p>
      ) : (
        <ul className="m-0 mt-3 list-none p-0">
          {patterns.map((p) => (
            <PatternRow key={p.id} pattern={p} onAnswer={onAnswer} onOpen={onOpen} />
          ))}
        </ul>
      )}

      {hidden.length > 0 && (
        <div className="mt-4">
          <TextButton onClick={() => setHiddenOpen((v) => !v)} aria-expanded={hiddenOpen}>
            {hidden.length === 1 ? '1 pattern you said wasn’t right' : `${hidden.length} patterns you said weren’t right`}
          </TextButton>
          {hiddenOpen && (
            <ul className="m-0 list-none p-0">
              {hidden.map((p) => (
                <li key={p.id} className="flex flex-wrap items-baseline gap-x-4 border-b border-line py-2.5">
                  <span className="text-[15px] text-muted line-through decoration-faint">{p.sentence}</span>
                  <TextButton className="h-8 text-[13px]" onClick={() => onAnswer(p, 'show')}>
                    Show it again
                  </TextButton>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}
    </Section>
  )
}

function PatternRow({
  pattern: p,
  onAnswer,
  onOpen,
}: {
  pattern: TastePattern
  onAnswer: (p: TastePattern, answer: 'right' | 'not_really') => void
  onOpen: (id: number) => void
}) {
  const [open, setOpen] = useState(false)
  const reasons = useQuery({ queryKey: ['stop-reasons'], queryFn: api.stopReasons, staleTime: Infinity, enabled: open })
  const label = (code: string) => reasons.data?.find((r) => r.code === code)?.label.toLowerCase() ?? code
  return (
    <li className="border-b border-line py-4">
      <p className="m-0 text-[17px] leading-[1.45] text-ink">{p.sentence}</p>
      <p className="m-0 mt-1 text-[14px] text-muted">
        {p.summary} · {p.sure ? 'sure' : 'fairly sure'}
      </p>
      <div className="mt-1 flex flex-wrap items-center gap-x-5">
        <TextButton onClick={() => setOpen((v) => !v)} aria-expanded={open}>
          {open ? 'Hide the games' : 'See the games'}
        </TextButton>
        <TextButton onClick={() => onAnswer(p, 'right')}>That’s right</TextButton>
        <TextButton onClick={() => onAnswer(p, 'not_really')}>Not really</TextButton>
      </div>
      {open && (
        <ul className="m-0 mt-1 max-w-[760px] list-none p-0 text-[14px]">
          {p.games.map((g) => (
            <li key={g.id} className="flex flex-wrap items-baseline gap-x-3 border-t border-line py-1.5">
              <button type="button" onClick={() => onOpen(g.id)} className="text-left text-ink hover:text-accent">
                {g.name}
              </button>
              <span className="text-muted">
                {OUTCOME[g.outcome]}
                {g.at ? `, ${dayText(g.at)}` : ''}
                {g.reasons.length ? `, for ${andList(g.reasons.map(label))}` : ''}
              </span>
              <span className="flex-1" />
              <span className="mono" title="How much it counts now">
                {g.weight >= 0.995 ? 'counts fully' : g.weight < 0.005 ? 'counts almost nothing' : `counts ${Math.round(g.weight * 100)}%`}
              </span>
            </li>
          ))}
        </ul>
      )}
    </li>
  )
}
