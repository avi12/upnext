import type { Status } from '../../types'
import { cx } from '../../lib/format'
import { STATUS_DOT, STATUS_WORD } from './status'

/** Status as a coloured dot and a word. */
export function StatusDot({ status, className }: { status: Status; className?: string }) {
  return (
    <span className={cx('inline-flex items-center gap-2', className)}>
      <span aria-hidden="true" className={cx('inline-block h-[7px] w-[7px] shrink-0 rounded-full', STATUS_DOT[status])} />
      {STATUS_WORD[status]}
    </span>
  )
}
