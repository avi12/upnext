import type { Settings } from '../../types'
import { Checkbox, Row } from './parts'
import type { SaveFn } from './useSettingsSave'

/** The stores a Galaxy sync has brought games from, each one kept unless
 *  unticked here (`galaxy_skip`). Galaxy reaches the other stores through
 *  plugins that can disconnect or disappear, and it calls unlinked ones
 *  connected too, so when Galaxy last added a game is shown instead. */
export function GalaxyStoresRow({ s, save }: { s: Settings; save: SaveFn }) {
  const seen = s.galaxy_integrations ?? []
  const others = seen.filter((i) => i.platform !== 'gog')
  if (!others.length) return null
  const skip = s.galaxy_skip ?? []
  const lastSync = Math.max(...seen.map((i) => i.seen_at))
  const toggle = (platform: string, keep: boolean) => {
    const next = keep ? skip.filter((p) => p !== platform) : [...skip, platform]
    const label = seen.find((i) => i.platform === platform)?.label ?? platform
    return save({ galaxy_skip: next.join(',') }, keep ? `${label} games kept` : `${label} games left out`)
  }
  return (
    <Row
      name="Stores through Galaxy"
      tag="optional"
      wrap
      desc="Untick a store you no longer use. Its games go at the next sync; nothing goes unless you untick it."
    >
      <div className="space-y-0.5 pb-1">
        {others.map((i) => {
          const notes = [
            `${i.games.toLocaleString()} ${i.games === 1 ? 'game' : 'games'}`,
            i.last_added ? `last added ${i.last_added}` : null,
            i.connected === false ? 'not connected in Galaxy' : null,
            i.seen_at < lastSync ? 'not in Galaxy at the last sync; its games are kept' : null,
          ]
          return (
            <Checkbox key={i.platform} checked={!skip.includes(i.platform)} onChange={(keep) => void toggle(i.platform, keep)} note={notes.filter(Boolean).join(' · ')}>
              {i.label}
            </Checkbox>
          )
        })}
      </div>
    </Row>
  )
}
