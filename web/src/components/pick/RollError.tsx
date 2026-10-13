import { Link } from 'react-router'

/** In the lineup's place when curating failed and no lineup is showing: an
 *  empty library, too few games for a discovery mode or the stores picked,
 *  or the AI failing, each with what to do next. */
export function RollError({
  err,
  dice = false,
  onRetry,
  onWithoutAi,
}: {
  err: { status: number; message: string }
  /** It was the dice's random draw: one game, never the AI. */
  dice?: boolean
  onRetry: () => void
  /** Offered when the AI failed and the request used it. */
  onWithoutAi: (() => void) | null
}) {
  const emptyLibrary = err.status === 400 && err.message.includes('library is empty')
  // "New to me" or "A hidden gem" with too few games: the server says which and why.
  const fewInMode = err.status === 400 && /of your games (is|are) (new to you|a hidden gem|hidden gems|off your radar)/.test(err.message)
  const fewText = err.message.replace(/^no candidates: /, '')
  return (
    <div className="flex flex-col items-start gap-3 border-t border-line pt-7">
      <h2 className="font-display text-xl font-bold">
        {emptyLibrary
          ? 'Nothing to pick from yet'
          : fewInMode
            ? 'Not enough to pick from'
            : err.status === 400
              ? 'Nothing left to pick'
              : dice
                ? "Rolling the dice didn't work"
                : "Curating didn't work"}
      </h2>
      <p className="max-w-2xl text-[15px] leading-relaxed break-words text-muted">
        {emptyLibrary
          ? 'Your library is empty. Sync it in Settings first.'
          : fewInMode
            ? `${fewText[0].toUpperCase()}${fewText.slice(1)}. ${dice ? 'Try more stores.' : 'Try fewer picks or more stores.'}`
            : err.status === 400
            ? "No games left to pick from match the stores you picked, or there aren't enough of them for this many picks. Try more stores or fewer picks."
            : err.message}
      </p>
      {emptyLibrary ? (
        <Link to="/settings#data" className="btn-outline">
          Sync your library in Settings
        </Link>
      ) : (
        <div className="flex flex-wrap gap-3">
          <button type="button" className="btn-outline" onClick={onRetry}>
            Try again
          </button>
          {err.status !== 400 && onWithoutAi && (
            <button type="button" className="btn-outline" onClick={onWithoutAi}>
              Pick without AI
            </button>
          )}
        </div>
      )}
    </div>
  )
}
