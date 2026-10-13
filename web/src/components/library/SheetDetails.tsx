import { Fragment, useState, type ReactNode } from 'react'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import type { GameView } from '../../types'
import { minutesToText, relativeTime, storeName } from '../../lib/format'
import { useMerge, useSplit } from './sheetActions'
import { storeVia } from './sheetText'
import { gameYear } from '../../lib/gameText'
import { useDebounced } from '../../lib/useDebounced'
import { Row, Section } from './SheetSections'
import { Place } from '../Place'

/** "Fold another title into this one": search, pick, confirm. */
function MergeTool({ game, onDone, onCancel }: { game: GameView; onDone: (g: GameView) => void; onCancel: () => void }) {
  const [text, setText] = useState('')
  const q = useDebounced(text.trim(), 250)
  const [chosen, setChosen] = useState<GameView | null>(null)
  const { data, isFetching } = useQuery({
    queryKey: ['merge-search', q],
    queryFn: () => api.games({ q, limit: 10 }),
    enabled: q.length >= 2,
  })
  const results = (data?.items ?? []).filter((g) => g.id !== game.id)
  const merge = useMerge(game, onDone)

  if (chosen) {
    return (
      <div className="flex flex-col gap-3 border-t border-line pt-4">
        <p className="m-0 text-[15px] leading-[1.6]">
          Fold <span className="font-semibold">{chosen.name}</span> ({chosen.copies.length}{' '}
          {chosen.copies.length === 1 ? 'copy' : 'copies'}) into <span className="font-semibold">{game.name}</span>? Its
          status, play time and pick history come along. This survives re-syncs, and you can split copies off again later.
        </p>
        <div className="flex flex-wrap items-center gap-3">
          <button type="button" className="btn-outline h-11" disabled={merge.isPending} onClick={() => merge.mutate(chosen)}>
            {merge.isPending ? 'Merging…' : 'Merge them'}
          </button>
          <button type="button" className="btn-quiet min-h-11 px-2" disabled={merge.isPending} onClick={() => setChosen(null)}>
            Pick another
          </button>
          <button type="button" className="btn-quiet min-h-11 px-2" disabled={merge.isPending} onClick={onCancel}>
            Never mind
          </button>
        </div>
      </div>
    )
  }

  return (
    <div className="flex flex-col gap-2 border-t border-line pt-4">
      <label htmlFor="merge-search" className="text-[15px] font-semibold">
        Which title should fold into {game.name}?
      </label>
      <div className="flex items-center gap-3">
        <input
          id="merge-search"
          autoFocus
          type="search"
          className="field"
          placeholder="Search by name"
          value={text}
          onChange={(e) => setText(e.target.value)}
        />
        <button type="button" className="btn-quiet min-h-11 shrink-0 px-2" onClick={onCancel}>
          Never mind
        </button>
      </div>
      <div className="max-h-64 overflow-y-auto" aria-live="polite">
        {q.length < 2 && <p className="m-0 py-2 text-[13.5px] text-dim">Type two letters or more.</p>}
        {q.length >= 2 && !isFetching && results.length === 0 && (
          <p className="m-0 py-2 text-[13.5px] text-dim">No other titles match.</p>
        )}
        <ul className="m-0 list-none p-0">
          {results.map((g) => (
            <li key={g.id}>
              <button
                type="button"
                onClick={() => setChosen(g)}
                className="flex min-h-11 w-full items-center justify-between gap-3 rounded-lg px-2 text-left hover:bg-raised"
              >
                <span className="truncate">{g.name}</span>
                <span className="shrink-0 font-mono text-[10.5px] text-dim">
                  {(g.owned_on?.length ? g.owned_on : [g.source]).map(storeName).join(', ')}
                  {gameYear(g) ? ` · ${gameYear(g)}` : ''}
                </span>
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  )
}

/** Developers, release, genres, the store copies (with split) and the merge tool. */
export function Details({ game, onUpdated, extra }: { game: GameView; onUpdated: (g: GameView) => void; extra?: ReactNode }) {
  const [merging, setMerging] = useState(false)
  const split = useSplit(onUpdated)
  const multi = game.copies.length > 1
  const preferredId = game.copies.find((c) => c.source === game.primary_source)?.game_id

  return (
    <Section title="Details">
      <dl className="m-0 flex flex-col">
        {game.developers.length > 0 && <Row label="Made by">{game.developers.join(', ')}</Row>}
        {(game.release_date || game.first_year) && (
          <Row label="Released">
            {game.release_date ?? game.first_year}
            {game.first_year && game.release_date && !game.release_date.includes(String(game.first_year)) && (
              <span className="text-muted"> (first out in {game.first_year})</span>
            )}
          </Row>
        )}
        {game.genres.length > 0 && <Row label="Genres">{game.genres.join(', ')}</Row>}
        {game.metacritic != null && <Row label="Metacritic">{game.metacritic}</Row>}
        <Row label={multi ? 'Your copies' : 'Your copy'}>
          <ul className="m-0 flex list-none flex-col gap-2 p-0">
            {game.copies.map((c) => (
              <li key={c.game_id} className="flex flex-wrap items-center gap-x-4 gap-y-1">
                <span className="min-w-0">
                  <span className="font-semibold">{storeVia(c)}</span>
                  {c.edition && <span className="text-muted"> · {c.edition}</span>}
                  {multi && c.game_id === preferredId && <span className="text-muted"> · preferred</span>}
                  <span className="block font-mono text-[10.5px] text-dim">
                    {c.installed ? 'Installed · ' : ''}
                    {c.playtime_known ? (c.playtime_minutes > 0 ? `${minutesToText(c.playtime_minutes)} played` : 'Never played') : 'Play time not tracked'}
                    {c.last_played ? ` · ${relativeTime(c.last_played)}` : ''}
                    {c.name && c.name !== game.name ? ` · listed as “${c.name}”` : ''}
                  </span>
                </span>
                <span className="ml-auto flex items-center gap-4 text-[14px]">
                  {c.run_url && (
                    <a href={c.run_url} onClick={() => void api.diaryLaunch(game.id)} className="btn-quiet min-h-9 font-semibold text-ink">
                      Play
                    </a>
                  )}
                  {c.store_url && (
                    <a href={c.store_url} target="_blank" rel="noreferrer" className="btn-quiet min-h-9">
                      Store
                    </a>
                  )}
                  {multi && (
                    <button
                      type="button"
                      className="btn-quiet min-h-9"
                      title="Make this copy its own title"
                      disabled={split.isPending}
                      onClick={() => split.mutate({ titleId: game.id, gameId: c.game_id })}
                    >
                      Split off
                    </button>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </Row>
      </dl>
      {game.steam_matched && (
        <p className="m-0 text-[13px] text-dim">
          Reviews, genres and tags were matched to this game by name on Steam, so they might describe another edition.
        </p>
      )}
      {!game.enriched && (
        <p className="m-0 text-[13px] text-warn">No details fetched for this one yet. Fetch details, in <Place section="Jobs" />, fills them in.</p>
      )}
      {extra}
      {merging ? (
        <MergeTool
          game={game}
          onCancel={() => setMerging(false)}
          onDone={(g) => {
            setMerging(false)
            onUpdated(g)
          }}
        />
      ) : (
        <button
          type="button"
          className="btn-quiet min-h-11 self-start text-[14px] underline decoration-underline underline-offset-4"
          onClick={() => setMerging(true)}
        >
          Fold another title into this one
        </button>
      )}
    </Section>
  )
}

/** Credits for the data sources. RAWG's free tier needs a visible backlink wherever its data shows. ProtonDB's
 * ratings only show with Linux compatibility on. */
export function DataLine({ game, linuxActive }: { game: GameView; linuxActive: boolean }) {
  const links = [
    ['RAWG', game.rawg_url],
    ['PCGamingWiki', game.pcgw_url],
    ['ProtonDB', linuxActive && game.appid && game.protondb_tier ? `https://www.protondb.com/app/${game.appid}` : null],
    ['Wikidata', game.wikidata_url],
    ['HowLongToBeat', game.hltb_url],
  ].filter((l): l is [string, string] => !!l[1])
  if (links.length === 0) return null
  return (
    <p className="m-0 text-[13px] text-dim">
      Info from{' '}
      {links.map(([label, url], i) => (
        <Fragment key={label}>
          {i > 0 && (i === links.length - 1 ? ' and ' : ', ')}
          <a href={url} target="_blank" rel="noreferrer" className="text-muted underline decoration-underline underline-offset-4 hover:text-ink">
            {label}
          </a>
        </Fragment>
      ))}
    </p>
  )
}
