import { cx } from '../lib/format'

export function Skeleton({ className }: { className?: string }) {
  return <div className={cx('skeleton', className)} />
}
