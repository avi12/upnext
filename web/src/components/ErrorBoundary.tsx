import { Component, type ErrorInfo, type ReactNode } from 'react'
import { page, reloadOnce, reloading, scriptMissing } from '../lib/reload'

interface Props {
  children: ReactNode
  /** A new value clears a failure: the address, so another page can open. */
  resetKey?: string
}

interface State {
  error: Error | null
  /** The `resetKey` the failure happened under. */
  key?: string
}

/** What shows when a page fails to render or to load, in place of a blank
 *  window (code health B-WEB-1): a plain sentence and Reload. A page whose
 *  script is gone after an upgrade loads itself again once first. */
export class ErrorBoundary extends Component<Props, State> {
  state: State = { error: null, key: this.props.resetKey }

  static getDerivedStateFromError(error: unknown): Partial<State> {
    return { error: error instanceof Error ? error : new Error(String(error)) }
  }

  static getDerivedStateFromProps(props: Props, state: State): Partial<State> | null {
    return props.resetKey !== state.key ? { error: null, key: props.resetKey } : null
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(error, info.componentStack)
    if (scriptMissing(error) && reloadOnce()) this.forceUpdate()
  }

  render() {
    const { error } = this.state
    if (!error) return this.props.children
    if (reloading()) {
      return (
        <p role="status" className="mx-auto max-w-[640px] px-6 py-16 text-[15px] text-muted">
          Loading the new version of UpNext…
        </p>
      )
    }
    const updated = scriptMissing(error)
    return (
      <div role="alert" className="mx-auto max-w-[640px] px-6 py-16">
        <p className="m-0 text-[15px] text-ink">
          {updated ? 'UpNext was updated while this page was open, and this page is from the version before.' : 'Something went wrong on this page.'}
        </p>
        <p className="m-0 mt-2 text-[13px] text-muted">
          {updated ? 'Reload to use the new version.' : 'Reloading usually helps. If it happens again, the details below say what failed.'}
        </p>
        {!updated && <p className="mono mt-2 break-words">{error.message}</p>}
        <button type="button" className="btn-accent mt-4" onClick={() => page.reload()}>
          Reload
        </button>
      </div>
    )
  }
}
