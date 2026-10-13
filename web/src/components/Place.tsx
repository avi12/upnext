import { Link } from 'react-router'
import { cx } from '../lib/format'
import { SETTINGS_SECTIONS, placeText, type SettingsSection } from '../lib/place'

/** A section of Settings named as a path, "Settings › Jobs", and linked to it,
 *  so a sentence that sends you somewhere also takes you there. */
export function Place({ section, className }: { section: SettingsSection; className?: string }) {
  return (
    <Link
      to={`/settings#${SETTINGS_SECTIONS[section]}`}
      className={cx('underline decoration-underline underline-offset-4 hover:text-ink', className)}
    >
      {placeText(section)}
    </Link>
  )
}
