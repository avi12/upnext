import type { Settings } from '../../types'
import { RowGrid, Section } from './parts'
import type { SaveFn } from './useSettingsSave'

/** The tray icon's two settings, shown only when UpNext runs on Windows. */
export function WindowsSection({ s, save }: { s: Settings; save: SaveFn }) {
  return (
    <Section id="windows" title="Windows" desc="UpNext's icon by the clock opens the window again; Quit in its menu stops UpNext.">
      <RowGrid>
        <Switch
          name="Keep running in the background"
          desc="Closing the window leaves UpNext running, so its jobs keep their schedules"
          checked={s.keep_running}
          onChange={(on) => void save({ keep_running: on }, on ? 'UpNext keeps running when the window closes' : 'Closing the window stops UpNext')}
        />
        <Switch
          name="Start with Windows"
          desc="UpNext starts when you sign in, with only its icon by the clock"
          checked={s.start_with_windows}
          onChange={(on) => void save({ start_with_windows: on }, on ? 'UpNext starts with Windows' : "UpNext doesn't start with Windows")}
        />
      </RowGrid>
    </Section>
  )
}

function Switch({ name, desc, checked, onChange }: { name: string; desc: string; checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <label className="flex cursor-pointer items-center justify-between gap-4 border-b border-line py-3.5">
      <span className="min-h-11 content-center">
        <span className="block text-[15.5px] font-bold">{name}</span>
        <span className="mt-0.5 block text-[13.5px] text-dim">{desc}</span>
      </span>
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} className="m-0 h-[18px] w-[18px] accent-accent" />
    </label>
  )
}
