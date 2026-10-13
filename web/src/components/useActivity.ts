import { createContext, useContext } from 'react'

export interface ActivityApi {
  /** Open the Activity sheet, with a run's log open when given its id. */
  open: (logFor?: number) => void
  close: () => void
  isOpen: boolean
  /** The newest finished run Activity has shown. */
  seenId: number
}

export const ActivityContext = createContext<ActivityApi | null>(null)

export function useActivity(): ActivityApi {
  const ctx = useContext(ActivityContext)
  if (!ctx) throw new Error('useActivity must be used inside ActivityProvider')
  return ctx
}
