import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import { absoluteTime, libraryStores, plural, relativeTime, storeName } from '../../lib/format'
import { Row, RowGrid, Section } from './parts'
import { SharedDataRow } from './SharedData'
import { VerdictsRow } from './VerdictsRow'
import { CommunityVerdictsRow } from './CommunityVerdictsRow'
import { useProfile } from '../../lib/queries'

function pct(n: number | undefined, total: number) {
  return total > 0 ? `${Math.round(((n ?? 0) / total) * 100)}%` : '–'
}

/** What the library holds and how much of it has details. Syncing and fetching are in Jobs. */
export function LibraryDataSection() {
  const stats = useQuery({ queryKey: ['stats'], queryFn: api.stats, staleTime: 30_000 })
  const profile = useProfile({ staleTime: 30_000 })

  const st = stats.data
  const stores = st ? libraryStores(st.by_source) : []
  const desc = st ? `${plural(st.total, 'game')} on ${plural(stores.length, 'store')}. Sync and fetch them under Jobs.` : undefined
  const lastSync = profile.data?.last_sync

  return (
    <Section id="data" title="Library data" desc={desc}>
      <RowGrid>
        <Row
          name={
            <>
              Libraries{' '}
              <span className="text-[13.5px] font-normal text-dim" title={lastSync ? absoluteTime(lastSync) : undefined}>
                · synced {relativeTime(lastSync)}
              </span>
            </>
          }
          desc={
            st ? (
              <span className="font-mono text-[10.5px]">
                {stores.map((k) => `${storeName(k)} ${(st.by_source[k] ?? 0).toLocaleString()}`).join(' · ')}
              </span>
            ) : undefined
          }
        />
        <Row
          name={
            <>
              Game details{' '}
              {st && (
                <span className="text-[13.5px] font-normal text-dim">
                  · {st.enriched.toLocaleString()} of {st.total.toLocaleString()} filled in
                </span>
              )}
            </>
          }
          desc={st ? (st.unenriched > 0 ? `${plural(st.unenriched, 'game still needs', 'games still need')} details` : 'Every game has store details') : undefined}
        />
      </RowGrid>
      <SharedDataRow />
      <VerdictsRow />
      <CommunityVerdictsRow />

      {/* Coverage */}
      <div className="flex flex-wrap items-baseline gap-x-5 gap-y-1 border-b border-line py-3.5">
        <span className="text-[13.5px] text-dim">Coverage</span>
        {st &&
          (
            [
              ['Tags', st.with_tags],
              ['RAWG', st.with_rawg],
              ['Wikidata', st.with_xref],
              ['Reviews', st.with_reviews],
              ['Player reviews', st.with_review_texts],
              ['Summaries', st.with_digests],
              ['PCGamingWiki', st.with_pcgw],
              ['HLTB', st.with_hltb],
            ] as const
          ).map(([name, n]) => (
            <span key={name} className="text-sm text-muted" title={`${(n ?? 0).toLocaleString()} of ${st.total.toLocaleString()}`}>
              {name} <span className="font-mono text-[11.5px] text-ink">{pct(n, st.total)}</span>
            </span>
          ))}
      </div>
    </Section>
  )
}
