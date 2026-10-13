/** What "Hide this game" does, as docs/players.md's "Hiding games" says it:
 *  shown under Lists › Hidden by me with several players. */
const HIDING_HELP = [
  'Hide this game keeps a game out of sight. Just from me takes it out of your own library, search and picks; nobody else is affected. From everyone else keeps your copy out of every other player’s UpNext: they won’t see it in their library, search, picks or statistics, unless they own it themselves. From everyone does both.',
  'Hiding isn’t the same as Not for me: it doesn’t tell UpNext anything about your taste. Everything you hid is listed here, where you can unhide it; if you have a PIN, it asks for it again.',
  'It’s hidden in UpNext only, so anyone who can open this computer’s files can still find it. To keep a game out of UpNext completely, hide it in Steam.',
]

export function HidingHelp() {
  return (
    <details className="mt-6 max-w-[760px] text-[14.5px] text-muted">
      <summary className="min-h-10 cursor-pointer py-2 text-ink">What hiding does</summary>
      <div className="flex flex-col gap-2 pb-2">
        {HIDING_HELP.map((p) => (
          <p key={p.slice(0, 20)} className="m-0 leading-[1.6]">
            {p}
          </p>
        ))}
      </div>
    </details>
  )
}
