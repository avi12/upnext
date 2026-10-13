import { afterEach, beforeEach, describe, expect, it } from 'vitest'
import { nearest, type Box } from './geometry'
import { focusFirst, focusables, move, trackGroups } from './engine'
import { closeTop, pushBack } from './back'

const at = (left: number, top: number, w = 100, h = 100): Box => ({ left, top, right: left + w, bottom: top + h })

describe('nearest', () => {
  // A 3×3 grid of 100 px cells with 20 px gaps.
  const grid = [0, 1, 2].flatMap((r) => [0, 1, 2].map((c) => at(c * 120, r * 120)))
  const centre = grid[4]

  it('moves to the neighbour in each direction', () => {
    expect(nearest(centre, grid, 'up')).toBe(1)
    expect(nearest(centre, grid, 'down')).toBe(7)
    expect(nearest(centre, grid, 'left')).toBe(3)
    expect(nearest(centre, grid, 'right')).toBe(5)
  })

  it('finds nothing past the edge', () => {
    expect(nearest(grid[2], grid, 'right')).toBe(-1)
    expect(nearest(grid[0], grid, 'up')).toBe(-1)
  })

  it('prefers the item lined up below to a nearer one off to the side', () => {
    const from = at(0, 0, 100, 40)
    const below = at(0, 200, 100, 40)
    const aside = at(300, 60, 100, 40)
    expect(nearest(from, [aside, below], 'down')).toBe(1)
  })

  it('counts a box that overlaps a little as lying that way', () => {
    const from = at(0, 0, 100, 100)
    const overlapping = at(90, 0, 100, 100)
    expect(nearest(from, [overlapping], 'right')).toBe(0)
  })

  it('moves down from a wide button to the item under its middle', () => {
    const wide = at(0, 0, 600, 50)
    const row = [at(0, 100), at(250, 100), at(500, 100)]
    expect(nearest(wide, row, 'down')).toBe(1)
  })
})

/** Lays out buttons with fixed boxes, since jsdom doesn't lay anything out. */
function place(el: HTMLElement, b: Box) {
  el.getBoundingClientRect = () =>
    ({ ...b, x: b.left, y: b.top, width: b.right - b.left, height: b.bottom - b.top, toJSON: () => b }) as DOMRect
}

function button(parent: HTMLElement, name: string, b: Box): HTMLButtonElement {
  const el = document.createElement('button')
  el.textContent = name
  parent.appendChild(el)
  place(el, b)
  return el
}

describe('the focus engine', () => {
  let untrack: () => void
  beforeEach(() => {
    document.body.innerHTML = ''
    untrack = trackGroups()
  })
  afterEach(() => untrack())

  it('moves between buttons by their place on screen and skips hidden ones', () => {
    const a = button(document.body, 'a', at(0, 0))
    const b = button(document.body, 'b', at(120, 0))
    const hidden = button(document.body, 'hidden', at(60, 0, 0, 0))
    expect(focusables()).toEqual([a, b])
    expect(hidden.isConnected).toBe(true)
    a.focus()
    expect(move('right')).toBe(true)
    expect(document.activeElement).toBe(b)
    expect(move('right')).toBe(false)
    expect(document.activeElement).toBe(b)
  })

  it('focuses the first item, or the one marked first, when nothing is focused', () => {
    button(document.body, 'a', at(0, 0))
    const b = button(document.body, 'b', at(120, 0))
    b.dataset.focusFirst = ''
    expect(move('down')).toBe(true)
    expect(document.activeElement).toBe(b)
  })

  it('returns to the item a row had when focus comes back to it', () => {
    const top = button(document.body, 'request', at(0, 0, 360, 50))
    const row = document.createElement('div')
    row.dataset.focusGroup = 'lineup'
    document.body.appendChild(row)
    const cards = [0, 1, 2].map((i) => button(row, `card ${i}`, at(i * 120, 100)))
    top.focus()
    move('down')
    expect(document.activeElement).toBe(cards[1])
    move('right')
    expect(document.activeElement).toBe(cards[2])
    move('up')
    expect(document.activeElement).toBe(top)
    // Under the wide button's middle is card 1, but the row remembers card 2.
    move('down')
    expect(document.activeElement).toBe(cards[2])
  })

  it('keeps the focus inside an open sheet', () => {
    const page = button(document.body, 'page', at(0, 0))
    const sheet = document.createElement('div')
    sheet.dataset.focusScope = ''
    document.body.appendChild(sheet)
    const inside = button(sheet, 'inside', at(0, 200))
    page.focus()
    expect(focusables()).toEqual([inside])
    // The focus isn't in the sheet yet, so a move puts it there.
    expect(move('up')).toBe(true)
    expect(document.activeElement).toBe(inside)
    expect(move('up')).toBe(false)
    sheet.remove()
    expect(focusFirst()).toBe(true)
    expect(document.activeElement).toBe(page)
  })
})

describe('back', () => {
  it('closes the newest open thing first, then reports nothing open', () => {
    const closed: string[] = []
    const offSheet = pushBack(() => closed.push('sheet'))
    const offMenu = pushBack(() => closed.push('menu'))
    expect(closeTop()).toBe(true)
    expect(closed).toEqual(['menu'])
    offMenu()
    expect(closeTop()).toBe(true)
    expect(closed).toEqual(['menu', 'sheet'])
    offSheet()
    expect(closeTop()).toBe(false)
  })
})
