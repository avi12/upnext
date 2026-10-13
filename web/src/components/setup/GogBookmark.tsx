import { useEffect, useRef, useState } from 'react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import { bookmarklet, saveSignIn } from '../../lib/gogLink'
import { TextButton } from '../settings/parts'

/** GOG's sign-in where no window can open on the computer UpNext runs on
 *  (Docker, another device): the "Send to UpNext" bookmark.
 *  `back` is the page the sign-in returns to. `onPaste` shows the paste form,
 *  for a browser where the bookmark can't run. */
export function GogBookmark({ back, onPaste }: { back: string; onPaste?: () => void }) {
  const link = useRef<HTMLAnchorElement>(null)
  const [copied, setCopied] = useState<boolean | null>(null)
  // The bookmark carries this install's key, so only it can finish a sign-in here.
  const key = useQuery({ queryKey: ['gog-bookmark'], queryFn: api.gogBookmark, staleTime: Infinity })
  const href = key.data ? bookmarklet(window.location.origin, key.data.key) : null
  // React refuses a javascript: href in JSX, so the bookmark's is set here.
  useEffect(() => {
    if (href) link.current?.setAttribute('href', href)
    else link.current?.removeAttribute('href')
  }, [href])

  const start = useMutation({
    mutationFn: api.gogBrowser,
    onSuccess: (s) => {
      saveSignIn({ state: s.state, back, expires_at: s.expires_at })
      window.location.assign(s.auth_url)
    },
  })
  const copy = async () => {
    if (!href) return
    try {
      await navigator.clipboard.writeText(href)
      setCopied(true)
    } catch {
      setCopied(false)
    }
  }

  return (
    <div className="flex flex-col gap-3 pb-1 text-[13.5px] leading-relaxed text-muted">
      <ol className="m-0 flex list-decimal flex-col gap-2.5 pl-5">
        <li>
          Drag this to your bookmarks bar, once:{' '}
          <a
            ref={link}
            className="mx-1 inline-flex h-9 items-center rounded-sm border border-line-strong bg-raised px-3 font-semibold text-ink no-underline"
            onClick={(e) => e.preventDefault()}
            title="Drag me to the bookmarks bar"
          >
            Send to UpNext
          </a>
          <span className="block text-[12.5px] text-dim">
            It holds this UpNext's address and a key of its own, so only it can finish a sign-in here. Saved one before
            this version? Drag it again: the old one no longer works. No bookmarks bar (a phone)?{' '}
            <button
              type="button"
              className="underline decoration-underline underline-offset-2 hover:text-ink"
              disabled={!href}
              onClick={() => void copy()}
            >
              Copy its address
            </button>{' '}
            and save it as a bookmark.
            {copied === true && <span className="text-good"> Copied.</span>}
            {copied === false && <span className="text-warn"> This page can't copy; use the paste way below.</span>}
            {key.isError && (
              <span role="alert" className="block text-bad">
                {key.error instanceof ApiError ? key.error.message : String(key.error)}
              </span>
            )}
          </span>
        </li>
        <li>
          <button type="button" className="btn-outline" disabled={start.isPending} onClick={() => start.mutate()}>
            {start.isPending ? 'Opening GOG…' : 'Sign in on GOG'}
          </button>{' '}
          <span className="text-dim">in this tab.</span>
          {start.isError && (
            <span role="alert" className="block text-[12.5px] text-bad">
              {start.error instanceof ApiError ? start.error.message : String(start.error)}
            </span>
          )}
        </li>
        <li>
          On GOG's page after you sign in, click <b className="font-semibold text-ink">Send to UpNext</b>. It brings you back here,
          connected. Do it within ten minutes; the sign-in works once.
        </li>
      </ol>
      {onPaste && (
        <TextButton onClick={onPaste} className="self-start">
          Paste GOG's address instead
        </TextButton>
      )}
    </div>
  )
}
