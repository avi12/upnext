import type { DiaryState, HabitKey, ReactionWhat, StatementKind, TasteOutcome } from '../../types'
import { relativeTime } from '../../lib/format'
import { MISS_REASONS } from '../../lib/ratings'

/** How the evidence list says what a game was. */
export const OUTCOME: Record<TasteOutcome, string> = {
  favourite: 'a favourite',
  staple: 'one of your staples',
  beaten: 'beaten',
  played_through: 'played through',
  still_playing: 'still played',
  picked: 'picked and played',
  shelved: 'shelved',
  abandoned: 'abandoned',
  not_for_me: 'not for you',
  turned_down: 'turned down in picks',
}

export const KINDS: { value: StatementKind; label: string; plural: string; empty: string }[] = [
  { value: 'like', label: 'Like', plural: 'Likes', empty: 'Nothing yet.' },
  { value: 'dislike', label: 'Dislike', plural: 'Dislikes', empty: 'Nothing yet.' },
  { value: 'hard_no', label: 'Hard no', plural: 'Hard nos', empty: 'Nothing yet.' },
]

/** The choices each habit takes, in the order the server keeps them. */
export const HABIT_CHOICES: Partial<Record<HabitKey, { value: string; label: string }[]>> = {
  times: [
    { value: 'mornings', label: 'Mornings' },
    { value: 'afternoons', label: 'Afternoons' },
    { value: 'evenings', label: 'Evenings' },
    { value: 'late_nights', label: 'Late nights' },
  ],
  who: [
    { value: 'solo', label: 'On my own' },
    { value: 'partner', label: 'With a partner' },
    { value: 'friends', label: 'With friends' },
    { value: 'kids', label: 'With kids' },
  ],
  where: [
    { value: 'desk', label: 'At a desk' },
    { value: 'couch', label: 'On the couch' },
    { value: 'deck', label: 'On a Steam Deck' },
  ],
}

/** A session length not said yet, as the blank in its sentence. */
export const SESSION_BLANK = 'x hours'

export const SESSION_MINUTES: { value: string; label: string }[] = [
  { value: '', label: 'not said' },
  { value: '30', label: 'half an hour' },
  { value: '60', label: 'an hour' },
  { value: '90', label: 'an hour and a half' },
  { value: '120', label: 'two hours' },
  { value: '180', label: 'three hours' },
  { value: '240', label: 'four hours or more' },
]

/** What was done, in the reactions list. */
export function reactionText(what: ReactionWhat, text: string | null): string {
  switch (what) {
    case 'good_pick':
      return 'a good pick'
    case 'not_a_fit': {
      const why = MISS_REASONS.find((r) => r.value === text)?.label.toLowerCase()
      return why ? `not a fit: ${why}` : 'not a fit'
    }
    case 'chosen':
      return 'played from picks'
    case 'not_tonight':
      return 'not tonight'
    case 'too':
      return `it's too ${text ?? '…'}`
    case 'same_problem':
      return `taken off with another "too ${text ?? '…'}"`
    case 'not_for_me':
      return 'never recommend this'
    case 'started':
      return `started it: ${text ?? ''}`
    case 'second_chance':
      return text === 'willing' ? 'willing to try again' : text === 'still_no' ? 'still no' : 'not for me'
    case 'reasons':
      return 'why you stopped'
  }
}

/** "the story, the music and co-op" */
export function andList(items: string[]): string {
  if (items.length <= 1) return items.join('')
  return `${items.slice(0, -1).join(', ')} and ${items[items.length - 1]}`
}

/** "3 games", "1 game" */
export function games(n: number): string {
  return `${n.toLocaleString()} ${n === 1 ? 'game' : 'games'}`
}

/** A pill-free toggle used for reasons and habits. */
export const CHIP = 'min-h-8 rounded-full border px-3 text-[13px] leading-tight'
export const CHIP_ON = 'border-accent bg-accent text-accent-ink'
export const CHIP_OFF = 'border-line-strong text-ink hover:bg-raised'

/** What the play diary does, in one plain sentence, wherever it's offered. */
export const DIARY_SENTENCE =
  'Once an hour UpNext asks Steam how long you’ve played each game lately and notes the change, so it can tell weeknights from weekends, how long you usually play, and when you play with others.'

/** The diary's state in words. */
export function diaryText(d: DiaryState): string {
  if (!d.on) {
    return d.rows + d.launches
      ? `Off, so it records nothing new. It holds ${d.rows.toLocaleString()} ${d.rows === 1 ? 'hour' : 'hours'} of play over ${d.days.toLocaleString()} ${d.days === 1 ? 'day' : 'days'}, which still count until you delete them.`
      : `Off, and it holds nothing. ${DIARY_SENTENCE}`
  }
  const parts = [
    d.rows + d.launches
      ? `On: ${d.rows.toLocaleString()} ${d.rows === 1 ? 'hour' : 'hours'} of play over ${d.days.toLocaleString()} ${d.days === 1 ? 'day' : 'days'}${d.launches ? `, and ${d.launches.toLocaleString()} ${d.launches === 1 ? 'launch' : 'launches'} from the Play button` : ''}.`
      : 'On, and nothing recorded yet: the first hour of play shows after the next reading.',
    d.last_reading ? `Last read from Steam ${relativeTime(d.last_reading)}.` : '',
    d.paused ? `Paused: ${d.paused}.` : '',
    d.error ? `The last reading failed: ${d.error}.` : '',
    'Epic and Amazon games through Heroic report no playtime, so only their launches from UpNext show.',
  ]
  return parts.filter(Boolean).join(' ')
}
