import { cx } from '../../lib/format'
import { possessive } from '../../lib/players'

/** A player's PIN typed into a form: when removing them, or binding a
 *  token or a device to them, which acts as them with no PIN after. */
export function PinField({ name, value, onChange, className }: { name: string; value: string; onChange: (pin: string) => void; className?: string }) {
  return (
    <label className={cx('flex max-w-[260px] flex-col gap-1 text-[13.5px] text-dim', className)}>
      {possessive(name)} PIN
      <input
        className="field text-sm"
        type="password"
        inputMode="numeric"
        autoComplete="off"
        maxLength={8}
        value={value}
        onChange={(e) => onChange(e.target.value.replace(/\D/g, ''))}
      />
    </label>
  )
}
