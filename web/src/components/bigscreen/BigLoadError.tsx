/** A big screen list that didn't load: what failed and Try again, where it
 *  used to say the list was empty ("0 games") with no way to retry (code
 *  health B-WEB-4). */
export function BigLoadError({ what, error, retrying, onRetry }: { what: string; error: Error; retrying: boolean; onRetry: () => void }) {
  return (
    <div role="alert" className="flex flex-col items-start gap-[0.75rem]">
      <p className="m-0 text-[1.1rem]">Couldn't load {what}.</p>
      <p className="m-0 text-[1rem] text-muted">{error.message}</p>
      <button type="button" className="big-btn-outline" disabled={retrying} onClick={onRetry}>
        {retrying ? 'Trying again…' : 'Try again'}
      </button>
    </div>
  )
}
