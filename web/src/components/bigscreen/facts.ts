import type { GameView } from '../../types'
import { hoursToText, minutesToText } from '../../lib/format'
import { storesText } from '../pick/text'
import { STATUS_WORD } from '../library/status'

/** The facts that matter on a couch: how long, whether it's installed, how
 *  much you've played, and how well a pad works with it where Steam says. */
export function couchFacts(g: GameView): string[] {
  const facts: string[] = []
  const story = hoursToText(g.hltb_main)
  if (story) facts.push(`${story} to beat`)
  if (g.controller_support === 'full') facts.push('Full controller support')
  else if (g.controller_support === 'partial') facts.push('Partial controller support')
  facts.push(g.installed ? 'Installed' : `Not installed · ${storesText(g)}`)
  if (g.playtime_known && g.playtime_minutes > 0) facts.push(`${minutesToText(g.playtime_minutes)} played`)
  facts.push(STATUS_WORD[g.status])
  return facts
}
