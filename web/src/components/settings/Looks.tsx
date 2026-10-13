import type { Settings, ThemeName } from '../../types'
import { applyTheme } from '../../lib/theme'
import { cx } from '../../lib/format'
import { Row, RowGrid, Section, Segmented } from './parts'
import { everyones, usePlayers } from '../../lib/players'
import type { SaveFn } from './useSettingsSave'

const THEME_LABEL: Record<ThemeName, string> = { arcade: 'Arcade', ember: 'Ember', daylight: 'Daylight' }

// Preview swatches for themes that aren't active, so they can't come from the
// CSS variables (those only exist for the current <html data-theme>).
// Values mirror --ground and --accent in index.css.
const SWATCH: Record<ThemeName, { ground: string; accent: string }> = {
  arcade: { ground: '#101218', accent: '#ffc93c' },
  ember: { ground: '#151412', accent: '#ff7a45' },
  daylight: { ground: '#f7f3ec', accent: '#b53a15' },
}

export function LooksSection({ s, save }: { s: Settings; save: SaveFn }) {
  const themes = s.themes.length ? s.themes : (Object.keys(THEME_LABEL) as ThemeName[])

  const setTheme = async (t: ThemeName) => {
    const prev = s.theme
    applyTheme(t, s.theme_follow_system)
    const err = await save({ theme: t }, `Theme: ${THEME_LABEL[t]}`)
    if (err) applyTheme(prev, s.theme_follow_system)
  }
  const setFollow = async (on: boolean) => {
    applyTheme(s.theme, on)
    const err = await save({ theme_follow_system: on }, on ? 'Following your system' : 'Not following your system')
    if (err) applyTheme(s.theme, !on)
  }

  return (
    <Section id="linux" title="Linux and looks">
      <RowGrid>
        <LinuxRow s={s} save={save} />
        <Row
          name={<span id="theme-label">Theme</span>}
          right={
            <div role="radiogroup" aria-labelledby="theme-label" className="flex flex-wrap gap-x-[18px]">
              {themes.map((t) => {
                const on = s.theme === t
                return (
                  <label key={t} className="flex min-h-11 cursor-pointer items-center gap-2">
                    <input
                      type="radio"
                      name="theme"
                      value={t}
                      checked={on}
                      onChange={() => void setTheme(t)}
                      className="m-0 h-[17px] w-[17px] accent-accent"
                    />
                    <span aria-hidden className="flex">
                      <span className="h-[18px] w-[18px] rounded-full shadow-[inset_0_0_0_1px_var(--key)]" style={{ background: SWATCH[t].ground }} />
                      <span
                        className="-ml-1.5 h-[18px] w-[18px] rounded-full shadow-[0_0_0_2px_var(--ground)]"
                        style={{ background: SWATCH[t].accent }}
                      />
                    </span>
                    <span className={cx('text-[14.5px]', on && 'font-bold')}>{THEME_LABEL[t] ?? t}</span>
                  </label>
                )
              })}
            </div>
          }
        />
        <div className="hidden lg:block" />
        <label className="flex cursor-pointer items-center justify-between gap-4 border-b border-line py-3.5">
          <span className="min-h-11 content-center">
            <span className="block text-[15.5px] font-bold">Follow my system</span>
            <span className="mt-0.5 block text-[13.5px] text-dim">Use Daylight when the desktop is in light mode</span>
          </span>
          <input
            type="checkbox"
            checked={s.theme_follow_system}
            onChange={(e) => void setFollow(e.target.checked)}
            className="m-0 h-[18px] w-[18px] accent-accent"
          />
        </label>
      </RowGrid>
    </Section>
  )
}

/** Linux compatibility: Settings, and the setup wizard's extras on Linux. */
export function LinuxRow({ s, save }: { s: Settings; save: SaveFn }) {
  const { multi } = usePlayers()
  const linuxStatus =
    s.linux_mode === 'auto'
      ? s.linux_detected
        ? 'On, because this computer runs Linux'
        : "Off, because this computer doesn't run Linux"
      : s.linux_mode === 'on'
        ? 'On: picks favour games that run well here'
        : 'Off: no ProtonDB ratings or Linux hints'

  return (
    <Row
      name="Linux compatibility"
      tag={everyones(multi)}
      desc={linuxStatus}
      right={
        <Segmented
          label="Linux compatibility"
          value={s.linux_mode}
          options={(s.linux_modes.length ? s.linux_modes : ['auto', 'on', 'off']).map((m) => ({
            value: m,
            label: m === 'auto' ? 'Auto' : m === 'on' ? 'On' : 'Off',
          }))}
          onChange={(m) => void save({ linux_mode: m }, `Linux compatibility: ${m === 'auto' ? 'Auto' : m === 'on' ? 'On' : 'Off'}`)}
        />
      }
    />
  )
}
