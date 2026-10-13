/** The four ways the focus moves: arrow keys, a D-pad or a stick. */
export type Dir = 'up' | 'down' | 'left' | 'right'

/** An element's box on screen, as `getBoundingClientRect` gives it. */
export interface Box {
  left: number
  top: number
  right: number
  bottom: number
}

/** How much more a step sideways counts than a step in the direction asked:
 *  moving down should land on the item below, not on a nearer one off to the
 *  side of it. */
const SIDEWAYS = 3

/** Picks the box the focus should move to from `from` in `dir`: the nearest
 *  one that lies that way, where an item lined up with `from` beats one that
 *  is off to the side by more than the gap between them. -1 when nothing lies
 *  that way. */
export function nearest(from: Box, boxes: Box[], dir: Dir): number {
  let best = -1
  let bestScore = Infinity
  boxes.forEach((b, i) => {
    const s = score(from, b, dir)
    if (s < bestScore) {
      best = i
      bestScore = s
    }
  })
  return best
}

function score(from: Box, to: Box, dir: Dir): number {
  const fx = (from.left + from.right) / 2
  const fy = (from.top + from.bottom) / 2
  const tx = (to.left + to.right) / 2
  const ty = (to.top + to.bottom) / 2
  // It has to lie that way: its middle past ours, and its far edge past ours,
  // so a box that only overlaps ours a little still counts.
  const ahead =
    dir === 'right'
      ? tx > fx && to.right > from.right
      : dir === 'left'
        ? tx < fx && to.left < from.left
        : dir === 'down'
          ? ty > fy && to.bottom > from.bottom
          : ty < fy && to.top < from.top
  if (!ahead) return Infinity
  const horizontal = dir === 'left' || dir === 'right'
  // The gap in the direction asked (none when the boxes overlap that way).
  const gap = Math.max(
    0,
    dir === 'right' ? to.left - from.right : dir === 'left' ? from.left - to.right : dir === 'down' ? to.top - from.bottom : from.top - to.bottom,
  )
  // How far off to the side it is: nothing while the two share a row (or a
  // column), else the gap between them across the direction.
  const across = horizontal
    ? Math.max(0, Math.max(from.top, to.top) - Math.min(from.bottom, to.bottom))
    : Math.max(0, Math.max(from.left, to.left) - Math.min(from.right, to.right))
  // Between boxes equally near, the one whose middle lines up best.
  const offset = horizontal ? Math.abs(ty - fy) : Math.abs(tx - fx)
  return gap + across * SIDEWAYS + offset / 100
}
