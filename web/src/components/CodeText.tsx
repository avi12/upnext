/** Text from the server with `backticked` commands, shown as code. */
export function CodeText({ text }: { text: string }) {
  return (
    <>
      {text.split('`').map((part, i) =>
        i % 2 ? (
          <code key={i} className="font-mono text-[0.85em] text-ink">
            {part}
          </code>
        ) : (
          part
        ),
      )}
    </>
  )
}
