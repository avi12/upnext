import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import { PLAY_LATER_KEY } from '../../components/pick/useGameActions'
import { BigCoverGrid } from '../../components/bigscreen/BigCoverGrid'
import { Cover } from '../../components/Cover'
import { BigLoadError } from '../../components/bigscreen/BigLoadError'

/** Big screen's Lists: Play later, then the wishlist. A game on the wishlist
 *  isn't yours yet, so it opens its store page rather than a game page. */
export function BigListsPage() {
  const later = useQuery({ queryKey: PLAY_LATER_KEY, queryFn: api.playLater })
  const wish = useQuery({ queryKey: ['wishlist'], queryFn: api.wishlist, staleTime: 60_000 })
  const wishlist = wish.data?.enabled ? wish.data.wishlist : []

  return (
    <div className="flex flex-col gap-[2rem] pt-[1.5rem]">
      <h1 className="sr-only">Lists</h1>
      <section aria-labelledby="big-later" className="flex flex-col gap-[1rem]">
        <h2 id="big-later" className="m-0 font-display text-[1.6rem] font-bold">
          Play later
        </h2>
        {later.isError ? (
          <BigLoadError what="Play later" error={later.error} retrying={later.isFetching} onRetry={() => void later.refetch()} />
        ) : later.data?.length ? (
          <BigCoverGrid games={later.data} label="Play later" />
        ) : (
          <p className="m-0 text-[1.05rem] text-muted">{later.isPending ? 'Loading…' : 'Nothing saved yet. Save a pick for later from its menu.'}</p>
        )}
      </section>
      {wish.data?.enabled !== false && (
        <section aria-labelledby="big-wish" className="flex flex-col gap-[1rem]">
          <h2 id="big-wish" className="m-0 font-display text-[1.6rem] font-bold">
            Wishlist
          </h2>
          {wish.isError ? (
            <BigLoadError what="your wishlist" error={wish.error} retrying={wish.isFetching} onRetry={() => void wish.refetch()} />
          ) : wishlist.length ? (
            <ul aria-label="Wishlist" data-focus-group="wishlist" className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(9rem,1fr))] gap-[1.25rem] p-0">
              {wishlist.map((e) => (
                <li key={e.id}>
                  <a
                    href={`https://store.steampowered.com/app/${e.appid}/`}
                    target="_blank"
                    rel="noreferrer"
                    className="flex flex-col gap-[0.4rem] rounded-[0.8rem]"
                  >
                    <Cover
                      game={{ id: e.appid, name: e.name, appid: e.appid, art_square: e.portrait, header_image: e.header_image }}
                      titleSize="text-[1.05rem]"
                      className="w-full rounded-[0.8rem]"
                    />
                    <span className="line-clamp-2 text-[0.95rem] leading-snug font-semibold">{e.name}</span>
                  </a>
                </li>
              ))}
            </ul>
          ) : (
            <p className="m-0 text-[1.05rem] text-muted">{wish.isPending ? 'Loading…' : 'Your wishlist is empty.'}</p>
          )}
        </section>
      )}
    </div>
  )
}
