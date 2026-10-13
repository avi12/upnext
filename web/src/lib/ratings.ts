import type { GameView, MissReason } from '../types'

/** The words for 1 to 5 stars on your rating of a game. */
export const STAR_WORDS = ['Disliked it', 'Didn’t click', 'It was fine', 'Liked it', 'Loved it'] as const

/** Why a pick wasn't a fit, as a pick card offers them. */
export const MISS_REASONS: { value: MissReason; label: string }[] = [
  { value: 'wrong_length', label: 'Wrong length' },
  { value: 'wrong_mood', label: 'Wrong mood' },
  { value: 'wrong_genre', label: 'Wrong genre' },
  { value: 'played_out', label: 'Played out' },
]

/** Whether the game page offers a rating: once you've played the game or
 *  given it a status, or when it has one already. An untouched game
 *  doesn't ask. */
export function offersRating(g: GameView): boolean {
  return (g.playtime_known && g.playtime_minutes > 0) || g.override != null || g.my_rating != null
}
