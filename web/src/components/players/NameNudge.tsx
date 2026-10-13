import { useEffect } from 'react'
import { useLocation, useNavigate } from 'react-router'
import { memory, nudgeOnce, pickerNeeded, placeholderName, usePlayers } from '../../lib/players'
import { useToast } from '../useToast'

/** Once per player in a browser, for a household from before the wizard
 *  asked for a name: someone still called "Player 1", which
 *  "Who's playing?" shows to everyone, is told where to change it. Only
 *  with two or more players, since one player's name shows nowhere, and
 *  never over "Who's playing?" or on Settings itself. */
export function NameNudge() {
  const { data, current, multi, bound } = usePlayers()
  const toast = useToast()
  const navigate = useNavigate()
  const { pathname } = useLocation()
  const due = !!current && multi && !bound && placeholderName(current.name) && pathname !== '/settings' && !pickerNeeded(data, memory())
  useEffect(() => {
    if (!due || !current || !nudgeOnce(current.id)) return
    toast.info(`You’re “${current.name}” here`, 'That’s the name UpNext gave you. Settings › Players changes it to your own.', {
      label: 'Change it',
      onClick: () => navigate('/settings#players'),
    })
  }, [due, current, toast, navigate])
  return null
}
