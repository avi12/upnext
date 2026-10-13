import { useRef } from 'react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { api, ApiError } from '../../api'
import type { SettingsPatch } from '../../types'
import { useToast } from '../useToast'
import { keys } from '../../lib/queries'

/** Returns an error message, or null when the patch was saved. */
export type SaveFn = (patch: SettingsPatch, what?: string, opts?: { inline?: boolean }) => Promise<string | null>

/** Save a settings patch with a small toast. Text fields pass `inline` so the
 *  error shows next to the field instead of in a toast. */
export function useSettingsSave(): SaveFn {
  const qc = useQueryClient()
  const toast = useToast()
  // Quick changes (arrow keys through a segmented control) can send several
  // saves at once; only the newest reply may update the cache, so a slow
  // earlier one can't flip the setting back.
  const latest = useRef(0)
  const mutation = useMutation({
    // One at a time, in order, so the server ends on the last choice too.
    scope: { id: 'settings' },
    mutationFn: ({ patch }: { patch: SettingsPatch; seq: number }) => api.updateSettings(patch),
    onSuccess: (data, { patch, seq }) => {
      if (seq === latest.current) qc.setQueryData(keys.settings, data)
      qc.invalidateQueries({ queryKey: keys.config })
      qc.invalidateQueries({ queryKey: keys.profile })
      // These change which games count as unplayed or what the Linux column says.
      if ('sampled_minutes' in patch || 'linux_mode' in patch) {
        for (const key of ['games', 'stats', 'collections', 'play-later', 'game', 'history']) qc.invalidateQueries({ queryKey: [key] })
      }
      // Other players' verdicts show on games, pick cards and the library's filters.
      if ('verdicts_receive' in patch) {
        for (const key of ['games', 'game', 'history', 'community-verdicts', 'verdicts-community']) qc.invalidateQueries({ queryKey: [key] })
      }
      if ('wishlist' in patch) {
        for (const key of ['wishlist', 'taste-sources', 'scheduled', 'game']) qc.invalidateQueries({ queryKey: [key] })
      }
    },
  })
  return async (patch, what, opts) => {
    const seq = ++latest.current
    try {
      await mutation.mutateAsync({ patch, seq })
      if (seq === latest.current) toast.success('Saved', what)
      return null
    } catch (e) {
      const msg = e instanceof ApiError ? e.message : String(e)
      if (!opts?.inline) toast.error("Couldn't save that", msg)
      return msg
    }
  }
}
