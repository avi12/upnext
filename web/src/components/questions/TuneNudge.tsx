import { useState } from 'react'
import { Link } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'

const DISMISSED = 'upnext.tune-nudge-dismissed'

function dismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED) === '1'
  } catch {
    return false
  }
}

/** One line under the request box while the profile has nothing you said in
 *  it: the first-run questions are where first picks get good. Until plan
 *  8's wizard asks them on the first run. */
export function TuneNudge() {
  const [hidden, setHidden] = useState(dismissed)
  const sources = useQuery({ queryKey: ['taste-sources'], queryFn: api.tasteSources, staleTime: 60_000, enabled: !hidden })
  const s = sources.data
  if (hidden || !s || s.library.games === 0) return null
  const a = s.answers
  if (a.favourites + a.likes + a.dislikes + a.hard_nos + a.habits > 0) return null
  const dismiss = () => {
    try {
      localStorage.setItem(DISMISSED, '1')
    } catch {
      /* it comes back next time */
    }
    setHidden(true)
  }
  return (
    <p className="m-0 flex flex-wrap items-baseline gap-x-3 text-[14.5px] text-muted">
      <span>
        Picks fit better once UpNext knows what you love.{' '}
        <Link to="/profile/questions" className="font-semibold text-ink underline decoration-underline underline-offset-4 hover:decoration-accent">
          Tune your profile
        </Link>{' '}
        in a couple of minutes.
      </span>
      <button type="button" onClick={dismiss} className="text-dim underline decoration-underline underline-offset-4 hover:text-ink">
        Hide this
      </button>
    </p>
  )
}
