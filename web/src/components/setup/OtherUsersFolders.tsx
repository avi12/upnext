import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { SetupInfo } from '../../types'
import { keys } from '../../lib/queries'
import { useToast } from '../useToast'

/** Steam and Heroic folders in other users' home folders on this computer
 *  (`other_users`), which UpNext never reads by itself, since Steam and
 *  Heroic keep one sign-in per user. A Heroic folder becomes this player's
 *  Heroic with one click; Steam's, everyone's, says how to point UpNext at
 *  it. The server lists them only to the computer UpNext runs on. */
export function OtherUsersFolders({ setup, heroic: offerHeroic = true, steam = false }: { setup: SetupInfo; heroic?: boolean; steam?: boolean }) {
  const qc = useQueryClient()
  const toast = useToast()
  const accounts = useQuery({ queryKey: ['accounts'], queryFn: api.accounts })
  const use = useMutation({
    mutationFn: (dir: string) => {
      // Their own Heroic connection when they have one, else a new one.
      const mine = accounts.data?.connections.find((c) => c.kind === 'heroic')
      return mine ? api.setConnection(mine.id, { heroic_dir: dir }) : api.addConnection('heroic', { heroic_dir: dir })
    },
    onSuccess: (_, dir) => {
      toast.success('Heroic folder chosen', `The next sync reads your games from ${dir}.`)
      for (const key of [['setup'], ['accounts'], keys.settings]) void qc.invalidateQueries({ queryKey: key })
    },
    onError: (e) => toast.error('That folder wasn’t chosen', e instanceof ApiError ? e.message : String(e)),
  })
  const heroic = offerHeroic ? setup.stores.heroic.other_users : []
  const steamDirs = steam ? setup.stores.steam.other_users : []
  if (heroic.length === 0 && steamDirs.length === 0) return null
  return (
    <div className="flex flex-col gap-2 text-[14px]">
      {heroic.map((dir) => (
        <div key={dir} className="flex flex-wrap items-center gap-3">
          <span className="min-w-0 flex-1">
            Heroic is in another user’s folder on this computer,{' '}
            <span className="font-mono text-[13px] break-words">{dir}</span>. If it’s yours, UpNext can read your games from it.
          </span>
          <button type="button" className="btn-outline" disabled={use.isPending || !accounts.data} onClick={() => use.mutate(dir)}>
            That’s mine
          </button>
        </div>
      ))}
      {steamDirs.map((dir) => (
        <p key={dir} className="m-0 text-muted">
          Steam is in another user’s folder on this computer, <span className="font-mono text-[13px] break-words">{dir}</span>. To
          read it, name it in <span className="font-mono text-[13px]">UPNEXT_STEAM_DIR</span> in UpNext’s{' '}
          <span className="font-mono text-[13px]">.env</span> and start UpNext again.
        </p>
      ))}
    </div>
  )
}
