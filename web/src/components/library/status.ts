import type { GameView, Status } from '../../types'
import { minutesToText } from '../../lib/format'

/** Status words as the library shows them. */
export const STATUS_WORD: Record<Status, string> = {
  untouched: 'Not started',
  sampled: 'Sampled',
  playing: 'Playing',
  shelved: 'Shelved',
  played: 'Played',
  done: 'Beat it',
  still_playing: 'Still playing',
  burnt_out: 'Burnt out',
  abandoned: 'Abandoned',
  hidden: 'Not for me',
  not_game: 'Not a game',
}

export const STATUS_DOT: Record<Status, string> = {
  untouched: 'bg-underline',
  sampled: 'bg-dim',
  playing: 'bg-accent',
  shelved: 'bg-warn',
  played: 'bg-good',
  done: 'bg-good',
  still_playing: 'bg-accent',
  burnt_out: 'bg-warn',
  abandoned: 'bg-bad',
  hidden: 'bg-faint',
  not_game: 'bg-faint',
}

/** "Played 3h", "Sampled 4m", "Beat it 21h", "Not started": every status
 *  with the time played when a store reports some. */
export function statusShort(g: GameView): string {
  if (g.status !== 'untouched' && g.playtime_known && g.playtime_minutes > 0) {
    return `${STATUS_WORD[g.status]} ${minutesToText(g.playtime_minutes)}`
  }
  return STATUS_WORD[g.status]
}

