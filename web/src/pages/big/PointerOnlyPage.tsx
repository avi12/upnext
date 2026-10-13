import { useNavigate } from 'react-router'
import { setBigScreen } from '../../lib/bigscreen'

/** In big screen's place for a page made for a mouse and keyboard (History,
 *  Collections, the taste profile): it offers the normal layout. */
export function PointerOnlyPage({ name }: { name: string }) {
  const navigate = useNavigate()
  return (
    <div className="flex max-w-[44rem] flex-col items-start gap-[1rem] pt-[3rem]">
      <h1 className="m-0 font-display text-[1.8rem] font-bold">{name} works best with a mouse and keyboard</h1>
      <p className="m-0 text-[1.1rem] text-muted">It isn't part of big screen. Leave big screen to use it here, or go back to picking.</p>
      <div className="flex flex-wrap gap-[0.75rem]">
        <button type="button" className="big-btn-accent" data-focus-first="" onClick={() => setBigScreen(false)}>
          Leave big screen
        </button>
        <button type="button" className="big-btn-outline" onClick={() => navigate('/')}>
          Back to Curate
        </button>
      </div>
    </div>
  )
}
