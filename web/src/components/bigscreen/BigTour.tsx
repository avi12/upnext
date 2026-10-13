import { useState, type ReactNode } from 'react'
import type { PadAction } from '../../lib/gamepad/pad'
import { BigSheet } from './BigMenu'
import { Glyph } from './Glyph'

/** One stop of the big-screen tour. Its words name the buttons with
 *  glyphs, so they follow the pad in hand (Xbox letters, PlayStation shapes,
 *  or the keys before a pad is used). */
interface BigTourStop {
  title: string
  body: ReactNode
}

const G = ({ a }: { a: PadAction }) => <Glyph action={a} className="mx-[0.15em] align-[0.1em]" />

/** The tour for a controller. The normal tour points at
 *  parts of the page a mouse reaches; this one is about the buttons, since
 *  they're what someone new to big screen has to find. */
const BIG_TOUR: BigTourStop[] = [
  {
    title: 'Say what you’re up for',
    body: (
      <>
        Curate picks from the games you own and haven’t finished. <G a="request" /> opens the request: chips for how long you have, a
        mood, who you’re with. <G a="accept" /> chooses a chip, and Curate makes the lineup.
      </>
    ),
  },
  {
    title: 'Your lineup',
    body: (
      <>
        The D-pad or the left stick moves between the picks. <G a="accept" /> opens a game’s page, with Play; <G a="menu" /> opens its
        menu: Not tonight, Good pick, Not a fit.
      </>
    ),
  },
  {
    title: 'Back, and the other pages',
    body: (
      <>
        <G a="back" /> closes what’s open, then goes back a page. <G a="prevTab" /> and <G a="nextTab" /> step through Curate, Library,
        Lists, Help me decide and Settings.
      </>
    ),
  },
  {
    title: 'Long lists, and help',
    body: (
      <>
        <G a="pageUp" /> and <G a="pageDown" /> go a screen up or down in the Library. <G a="help" /> shows every button, with this
        tour and the way out of big screen.
      </>
    ),
  },
]

/** The big-screen tour, a stop at a time in a sheet. Back closes it. */
export function BigTour({ onClose }: { onClose: () => void }) {
  const [at, setAt] = useState(0)
  const stop = BIG_TOUR[at]
  const last = at === BIG_TOUR.length - 1
  return (
    <BigSheet title={stop.title} onClose={onClose} className="max-w-[40rem]">
      <p className="m-0 text-[1.1rem] leading-[1.7]">{stop.body}</p>
      <div className="flex flex-wrap items-center gap-[0.75rem]">
        <button type="button" className="big-btn-accent" data-focus-first="" onClick={() => (last ? onClose() : setAt(at + 1))}>
          {last ? 'Done' : 'Next'}
        </button>
        {at > 0 && (
          <button type="button" className="big-btn-outline" onClick={() => setAt(at - 1)}>
            Back
          </button>
        )}
        <span className="ml-auto text-[0.95rem] text-muted" aria-live="polite">
          {at + 1} of {BIG_TOUR.length}
        </span>
      </div>
    </BigSheet>
  )
}
