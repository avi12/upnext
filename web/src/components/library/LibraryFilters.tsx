import { useState } from 'react'
import type { ReviewVocabulary, Stats } from '../../types'
import { scaleOf } from '../../lib/reviewTags'
import { plural, storeName } from '../../lib/format'
import { SentenceSelect } from './SentenceSelect'
import { BACKLOG_CHOICES, LINUX_CHOICES, RATING_CHOICES, SORT_MENU, STATUS_CHOICES, sortPhrase, storeChoices, type LibrarySort, type Order } from './filters'
import { filterState, type LibraryParams } from './useLibraryParams'

/** The library's filter sentence: "Unplayed games from any library, best
 *  reviewed first", with More filters for genre, tag, installed and what
 *  the reviews say. Each word is a select that writes to the address. */
export function LibraryFilters({
  p,
  q,
  stats,
  vocab,
  linuxActive,
  total,
  update,
  setSort,
  clearFilters,
  backlogOn = false,
}: {
  p: LibraryParams
  /** The search, debounced. */
  q: string
  stats: Stats | undefined
  vocab: ReviewVocabulary | undefined
  linuxActive: boolean
  /** Games matching, once the first page is in. */
  total: number | null
  update: (patch: Record<string, string | null>) => void
  setSort: (s: LibrarySort, o: Order) => void
  clearFilters: () => void
  /** Other backlog players' verdicts are on: their filter shows. */
  backlogOn?: boolean
}) {
  const { status, store, linuxChoice, genre, tag, difficulty, sessions, mood, praised, notCriticized, installed, rating, backlog, sort, order, view } = p
  const [moreOpen, setMoreOpen] = useState(false)

  const statusOptions = STATUS_CHOICES.map((c) => {
    const n = c.staple
      ? (stats?.staples ?? null)
      : c.statuses
        ? c.statuses.reduce((sum, s) => sum + (stats?.by_status?.[s] ?? 0), 0)
        : (stats?.total ?? null)
    return { value: c.value, label: stats && n != null ? `${c.label} (${n.toLocaleString()})` : c.label }
  })
  const statusLabel =
    status === 'not_game' ? 'Non-games' : status === 'staples' ? 'Your staples' : (STATUS_CHOICES.find((c) => c.value === status)?.label ?? 'Everything')
  const statusTail = status === 'everything' || status === 'not_game' || status === 'staples' ? '' : ' games'

  const choices = storeChoices(stats?.by_source)
  if (store && !choices.some((c) => c.value === store)) choices.push({ value: store, label: storeName(store) })
  const storeOptions = choices.map((c) => {
    const n = c.value ? (stats?.by_source?.[c.value] ?? (stats ? 0 : undefined)) : undefined
    return { value: c.value, label: n != null ? `${c.label} (${n.toLocaleString()})` : c.label }
  })
  const storeLabel = choices.find((c) => c.value === store)?.label ?? 'any library'

  const linuxLabelText = LINUX_CHOICES.find((c) => c.value === linuxChoice)?.label ?? LINUX_CHOICES[0].label

  const sortMenu = SORT_MENU.some(([s, o]) => s === sort && o === order) ? SORT_MENU : [...SORT_MENU, [sort, order] as [LibrarySort, Order]]
  const phrase = sortPhrase(sort, order)
  const sortOptions = sortMenu.map(([s, o]) => {
    const ph = sortPhrase(s, o)
    return { value: `${s}:${o}`, label: `${ph.before}${ph.label}${ph.after}` }
  })

  const genreOptions = [
    { value: '', label: 'any genre' },
    ...(stats?.genres ?? []).map((g) => ({ value: g.name, label: `${g.name} (${g.count.toLocaleString()})` })),
  ]
  if (genre && !genreOptions.some((o) => o.value === genre)) genreOptions.push({ value: genre, label: genre })
  const tagOptions = [
    { value: '', label: 'anything' },
    ...(stats?.tags ?? []).slice(0, 150).map((t) => ({ value: t.name, label: `${t.name} (${t.count.toLocaleString()})` })),
  ]
  if (tag && !tagOptions.some((o) => o.value === tag)) tagOptions.push({ value: tag, label: tag })

  // Review-tag filters, with counts from the vocabulary.
  const facets: Record<string, string> = { difficulty, mood, sessions }
  const facetOptions = (name: string, any: string) => [
    { value: '', label: any },
    ...(scaleOf(vocab, name)?.values ?? [])
      .filter((v) => v.count > 0 || v.value === facets[name])
      .map((v) => ({ value: v.value, label: `${v.label.toLowerCase()} (${v.count.toLocaleString()})` })),
  ]
  const facetWord = (name: string, value: string, any: string) =>
    value ? (scaleOf(vocab, name)?.values.find((v) => v.value === value)?.label.toLowerCase() ?? value) : any
  const aspects = vocab?.aspects ?? []
  const praisedOptions = [
    { value: '', label: 'anything' },
    ...[...aspects]
      .filter((a) => a.praised > 0 || a.name === praised)
      .sort((a, b) => b.praised - a.praised)
      .map((a) => ({ value: a.name, label: `${a.name} (${a.praised.toLocaleString()})` })),
  ]
  const complaintOptions = [
    { value: '', label: 'any complaints are fine' },
    ...[...aspects]
      .filter((a) => a.criticized + a.divisive > 0 || a.name === notCriticized)
      .sort((a, b) => b.criticized + b.divisive - (a.criticized + a.divisive))
      .map((a) => ({ value: a.name, label: `no complaints about ${a.name}` })),
  ]
  const { reviewActive, extraActive, unfiltered, anyFilter } = filterState(p, q, linuxActive)
  // Few reviews speak to some scales (sessions: about 8% of titles), so say
  // how many games could match at all.
  const covered = [
    ...(['difficulty', 'mood', 'sessions'] as const)
      .filter((n) => facets[n])
      .map((n) => ({ what: n === 'sessions' ? 'session length' : n, n: scaleOf(vocab, n)?.known })),
    ...aspects.filter((a) => a.name === praised).map((a) => ({ what: a.name, n: a.praised + a.criticized + a.divisive })),
  ].filter((c): c is { what: string; n: number } => c.n != null)
  const coverage = [
    covered.length > 0 &&
      `Reviews speak to ${covered.map((c) => `${c.what} in ${c.n.toLocaleString()}`).join(' and ')} of your games.`,
    notCriticized && vocab && `Complaints are checked in the ${plural(vocab.tagged, 'game')} with review summaries.`,
    'Games their reviews don\'t cover are left out.',
  ]
    .filter(Boolean)
    .join(' ')

  const showMore = moreOpen || extraActive

  return (
    <div className="min-w-0 flex-1">
      <p className="m-0 text-[17px] leading-[1.9] text-muted">
        <SentenceSelect
          label="Which games"
          display={statusLabel}
          value={status}
          options={statusOptions}
          onChange={(v) => update({ status: v })}
        />
        {statusTail} from{' '}
        <SentenceSelect
          label="Store"
          display={storeLabel}
          value={store}
          options={storeOptions}
          onChange={(v) => update({ store: v || null })}
        />
        {linuxActive && (
          <>
            {' '}that{' '}
            <SentenceSelect
              label="Linux support"
              display={linuxLabelText}
              value={linuxChoice}
              options={LINUX_CHOICES}
              onChange={(v) => update({ linux: v || null })}
            />
          </>
        )}
        {', '}
        {phrase.before}
        <span data-tour="sort">
          <SentenceSelect
            label="Sort by"
            display={phrase.label}
            value={`${sort}:${order}`}
            options={sortOptions}
            onChange={(v) => {
              const [s, o] = v.split(':') as [LibrarySort, Order]
              setSort(s, o)
            }}
          />
        </span>
        {phrase.after}
        {q && (
          <>
            , matching <span className="text-ink">“{q}”</span>{' '}
            <button
              type="button"
              onClick={() => update({ q: null })}
              className="btn-quiet text-[14px] underline decoration-underline underline-offset-4"
            >
              clear
            </button>
          </>
        )}
        <span className="ml-2.5 whitespace-nowrap font-mono text-[11.5px] text-dim" aria-live="polite">
          {total != null ? `${total.toLocaleString()} ${total === 1 ? 'game' : 'games'}` : 'Loading…'}
          {view === 'table' && unfiltered && stats ? ` · ${stats.multi_store.toLocaleString()} on 2+ stores` : ''}
        </span>
        {!extraActive && (
          <button
            type="button"
            aria-expanded={showMore}
            aria-controls="library-more-filters"
            data-tour="more-filters-toggle"
            onClick={() => setMoreOpen((o) => !o)}
            className="btn-quiet ml-4 inline-flex min-h-11 align-middle text-[14px]"
          >
            {showMore ? 'Fewer filters' : 'More filters'}
          </button>
        )}
      </p>
      <div data-tour="more-filters">
        {showMore && (
          <p id="library-more-filters" className="m-0 mt-1 text-[15.5px] leading-[1.9] text-muted">
            In{' '}
            <SentenceSelect
              label="Genre"
              display={genre || 'any genre'}
              value={genre}
              options={genreOptions}
              onChange={(v) => update({ genre: v || null })}
            />
            , tagged{' '}
            <SentenceSelect
              label="Tag"
              display={tag || 'anything'}
              value={tag}
              options={tagOptions}
              onChange={(v) => update({ tag: v || null })}
            />
            ,{' '}
            <SentenceSelect
              label="Installed"
              display={installed ? 'installed only' : 'installed or not'}
              value={installed ? '1' : ''}
              options={[
                { value: '', label: 'installed or not' },
                { value: '1', label: `installed only${stats ? ` (${stats.installed.toLocaleString()})` : ''}` },
              ]}
              onChange={(v) => update({ installed: v || null })}
            />
            ,{' '}
            <SentenceSelect
              label="Your rating"
              display={RATING_CHOICES.find((c) => c.value === rating)?.label ?? RATING_CHOICES[0].label}
              value={rating}
              options={RATING_CHOICES.map((c) =>
                c.value === 'rated' && stats ? { ...c, label: `${c.label} (${stats.rated.toLocaleString()})` } : c,
              )}
              onChange={(v) => update({ rating: v || null })}
            />
            .
          </p>
        )}
        {showMore && (
          <p className="m-0 text-[15.5px] leading-[1.9] text-muted">
            Reviews say{' '}
            <SentenceSelect
              label="Difficulty"
              display={facetWord('difficulty', difficulty, 'any difficulty')}
              value={difficulty}
              options={facetOptions('difficulty', 'any difficulty')}
              onChange={(v) => update({ difficulty: v || null })}
            />
            ,{' '}
            <SentenceSelect
              label="Mood"
              display={facetWord('mood', mood, 'any mood')}
              value={mood}
              options={facetOptions('mood', 'any mood')}
              onChange={(v) => update({ mood: v || null })}
            />
            ,{' '}
            <SentenceSelect
              label="Session length"
              display={facetWord('sessions', sessions, 'any session length')}
              value={sessions}
              options={facetOptions('sessions', 'any session length')}
              onChange={(v) => update({ sessions: v || null })}
            />
            ; praised for{' '}
            <SentenceSelect
              label="Praised for"
              display={praised || 'anything'}
              value={praised}
              options={praisedOptions}
              onChange={(v) => update({ praised: v || null })}
            />
            ;{' '}
            <SentenceSelect
              label="Complaints"
              display={notCriticized ? `no complaints about ${notCriticized}` : 'any complaints are fine'}
              value={notCriticized}
              options={complaintOptions}
              onChange={(v) => update({ not_criticized: v || null })}
            />
            .
            {anyFilter && (
              <button type="button" onClick={clearFilters} className="btn-quiet ml-4 inline-flex min-h-11 align-middle text-[14px]">
                Reset filters
              </button>
            )}
          </p>
        )}
        {showMore && (backlogOn || backlog) && (
          <p className="m-0 text-[15.5px] leading-[1.9] text-muted">
            Backlog players say{' '}
            <SentenceSelect
              label="Backlog players say"
              display={BACKLOG_CHOICES.find((c) => c.value === backlog)?.label ?? BACKLOG_CHOICES[0].label}
              value={backlog}
              options={BACKLOG_CHOICES}
              onChange={(v) => update({ backlog: v || null })}
            />
            .
          </p>
        )}
        {reviewActive && vocab && <p className="m-0 max-w-[68ch] text-[13px] leading-[1.6] text-dim">{coverage}</p>}
      </div>
    </div>
  )
}
