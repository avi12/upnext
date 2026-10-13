/** What a gamepad's buttons do in big screen. Every action is also
 *  a key, so a keyboard, a TV remote or Steam Input's keyboard template does
 *  the same. */
export type PadAction =
  | 'up'
  | 'down'
  | 'left'
  | 'right'
  /** A: choose the focused item. */
  | 'accept'
  /** B: close what's open, else go back a page. */
  | 'back'
  /** X: the focused card's menu. */
  | 'menu'
  /** Y: the request. */
  | 'request'
  /** The bumpers: the tab before or after. */
  | 'prevTab'
  | 'nextTab'
  /** The triggers: a page up or down a long list. */
  | 'pageUp'
  | 'pageDown'
  /** Menu (Start): Settings. */
  | 'settings'
  /** View (Back, Select): the button help. */
  | 'help'

/** The Gamepad API's standard mapping, button by button
 *  (https://w3c.github.io/gamepad/#remapping). */
export const BUTTONS: Partial<Record<number, PadAction>> = {
  0: 'accept',
  1: 'back',
  2: 'menu',
  3: 'request',
  4: 'prevTab',
  5: 'nextTab',
  6: 'pageUp',
  7: 'pageDown',
  8: 'help',
  9: 'settings',
  12: 'up',
  13: 'down',
  14: 'left',
  15: 'right',
}

/** Held this long, a direction or a page starts repeating… */
export const REPEAT_AFTER = 400
/** …once every this many milliseconds. */
export const REPEAT_EVERY = 120
/** How far a stick or a trigger has to go to count as pressed. */
const PRESSED = 0.5
/** Within this of the middle, a stick is at its centre. */
const CENTRE = 0.25
/** How long a stick has to stay still at its centre before it may lean the
 *  other way: a trigger read as a stick's axis passes the centre on its way
 *  back to -1 far quicker than this. */
export const SETTLE = 100
/** Still means within this of where it came to the centre. A stick let go
 *  springs there and stays; a trigger let go slowly keeps moving through
 *  it, unless it takes over four seconds to let go. */
const STILL = 0.05

const REPEATS: ReadonlySet<PadAction> = new Set(['up', 'down', 'left', 'right', 'pageUp', 'pageDown'])

/** The parts of the Gamepad API's `Gamepad` that big screen reads, so tests
 *  can hand in plain objects. */
export interface PadState {
  id: string
  index: number
  connected?: boolean
  mapping?: string
  buttons: ReadonlyArray<{ pressed: boolean; value: number }>
  axes: ReadonlyArray<number>
}

/** The actions one pad holds down right now. `leans` says whether the left
 *  stick's axis (0 across, 1 down) counts as leaning at its value; by
 *  default it always does. */
export function held(pad: PadState, leans: (axis: number, value: number) => boolean = () => true): Set<PadAction> {
  const out = new Set<PadAction>()
  pad.buttons.forEach((b, i) => {
    const action = BUTTONS[i]
    if (action && (b.pressed || b.value > PRESSED)) out.add(action)
  })
  // The left stick moves like the D-pad, along whichever axis it leans on most.
  const [rawX = 0, rawY = 0] = pad.axes
  const x = leans(0, rawX) ? rawX : 0
  const y = leans(1, rawY) ? rawY : 0
  if (Math.max(Math.abs(x), Math.abs(y)) > PRESSED) {
    if (Math.abs(x) > Math.abs(y)) out.add(x > 0 ? 'right' : 'left')
    else out.add(y > 0 ? 'down' : 'up')
  }
  return out
}

/** Turns what the pads hold, read once per animation frame, into actions:
 *  one when a button goes down, then repeats for directions and pages held
 *  past `REPEAT_AFTER`. A button held through a page change doesn't act again
 *  until it's let go. */
export class PadReader {
  /** For each action held: when it next repeats (Infinity when it doesn't). */
  private down = new Map<PadAction, number>()
  /** Each stick axis of each pad: the side it may lean to (1 or -1), 0 when
   *  it rested at its centre and may lean either way, null until it has; and
   *  since when, and where, it has been still at its centre. */
  private sticks = new Map<string, { side: -1 | 0 | 1 | null; centred: { since: number; at: number } | null }>()

  /** Whether a stick's axis at `value` leans that way. An axis counts only
   *  once it has been seen at its centre, and leaning across to the other
   *  side counts only after it has stayed still there `SETTLE`. A pad whose layout
   *  the browser got wrong can put a trigger on a stick's axis, at -1 when
   *  let go (WebKitGTK with Microsoft's wireless adapter on Linux): that
   *  axis then never reads as a stick held up. */
  private leans(key: string, value: number, now: number): boolean {
    let s = this.sticks.get(key)
    if (!s) {
      // Its first reading is where it rests, if that's the centre.
      s = { side: Math.abs(value) < CENTRE ? 0 : null, centred: null }
      this.sticks.set(key, s)
    }
    if (Math.abs(value) < CENTRE) {
      if (!s.centred || Math.abs(value - s.centred.at) > STILL) s.centred = { since: now, at: value }
      if (now - s.centred.since >= SETTLE) s.side = 0
      return false
    }
    s.centred = null
    if (Math.abs(value) <= PRESSED) return false
    const side = value > 0 ? 1 : -1
    if (s.side === 0) s.side = side
    else if (s.side !== side) s.side = null
    return s.side === side
  }

  /** The actions to run for this frame, and the pad that sent the last one. */
  poll(pads: ReadonlyArray<PadState | null | undefined>, now: number): { actions: PadAction[]; from: PadState | null } {
    const holding = new Map<PadAction, PadState>()
    for (const pad of pads) {
      if (!pad || pad.connected === false) continue
      const leans = (axis: number, value: number) => this.leans(`${pad.index}:${pad.id}:${axis}`, value, now)
      for (const a of held(pad, leans)) if (!holding.has(a)) holding.set(a, pad)
    }
    const actions: PadAction[] = []
    let from: PadState | null = null
    for (const [a, pad] of holding) {
      const next = this.down.get(a)
      if (next === undefined) {
        actions.push(a)
        from = pad
        this.down.set(a, REPEATS.has(a) ? now + REPEAT_AFTER : Infinity)
      } else if (now >= next) {
        actions.push(a)
        from = pad
        this.down.set(a, now + REPEAT_EVERY)
      }
    }
    for (const a of this.down.keys()) if (!holding.has(a)) this.down.delete(a)
    return { actions, from }
  }

  /** Forget what's held, as if every button were let go. */
  reset() {
    this.down.clear()
    this.sticks.clear()
  }
}

/** Whose button names to show. */
export type PadStyle = 'xbox' | 'playstation' | 'deck'

/** The pad's maker, from the id the browser gives it: Sony's pads get
 *  PlayStation shapes and Valve's the Steam Deck's names; everything else,
 *  Steam Input's virtual pad included, gets Xbox letters. Chromium puts the
 *  USB vendor id in the name (054c Sony, 28de Valve); Firefox starts with it. */
export function padStyle(id: string): PadStyle {
  const s = id.toLowerCase()
  if (/sony|playstation|dualshock|dualsense|\b054c\b/.test(s)) return 'playstation'
  if (/valve|steam deck|\b28de\b/.test(s)) return 'deck'
  return 'xbox'
}

/** What each pad calls the buttons big screen uses. */
export const GLYPHS: Record<PadStyle, Record<PadAction, string>> = {
  xbox: {
    up: 'D-pad up',
    down: 'D-pad down',
    left: 'D-pad left',
    right: 'D-pad right',
    accept: 'A',
    back: 'B',
    menu: 'X',
    request: 'Y',
    prevTab: 'LB',
    nextTab: 'RB',
    pageUp: 'LT',
    pageDown: 'RT',
    settings: 'Menu',
    help: 'View',
  },
  playstation: {
    up: 'D-pad up',
    down: 'D-pad down',
    left: 'D-pad left',
    right: 'D-pad right',
    accept: '✕',
    back: '○',
    menu: '□',
    request: '△',
    prevTab: 'L1',
    nextTab: 'R1',
    pageUp: 'L2',
    pageDown: 'R2',
    settings: 'Options',
    help: 'Create',
  },
  deck: {
    up: 'D-pad up',
    down: 'D-pad down',
    left: 'D-pad left',
    right: 'D-pad right',
    accept: 'A',
    back: 'B',
    menu: 'X',
    request: 'Y',
    prevTab: 'L1',
    nextTab: 'R1',
    pageUp: 'L2',
    pageDown: 'R2',
    settings: 'Menu',
    help: 'View',
  },
}

/** The keys that do the same, for the help with no pad connected. */
export const KEYS: Record<PadAction, string> = {
  up: '↑',
  down: '↓',
  left: '←',
  right: '→',
  accept: 'Enter',
  back: 'Esc',
  menu: 'M',
  request: '/',
  prevTab: '[',
  nextTab: ']',
  pageUp: 'Page Up',
  pageDown: 'Page Down',
  settings: 'S',
  help: '?',
}
