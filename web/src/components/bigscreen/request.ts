import { upperFirst } from '../../lib/format'

/** The request as chips: how long, the mood, who with. Each chip
 *  is a few words of the request the AI reads, so nothing needs typing. */
export const GROUPS: { name: string; chips: [label: string, words: string][] }[] = [
  {
    name: 'How long',
    chips: [
      ['Half an hour', 'about half an hour'],
      ['An hour', 'about an hour'],
      ['A couple of hours', 'a couple of hours'],
      ['All evening', 'a whole evening'],
    ],
  },
  {
    name: 'Mood',
    chips: [
      ['Relaxing', 'something relaxing'],
      ['A good story', 'a good story'],
      ['Action', 'some action'],
      ['One I can finish', 'something I can actually finish'],
      // The server reads these two as its discovery modes.
      ['A hidden gem', 'a hidden gem'],
      ['New to me', 'new to me'],
    ],
  },
  {
    name: 'Who with',
    chips: [
      ['On my own', 'playing alone'],
      ['Friends online', 'with friends online'],
      ['Couch co-op', 'couch co-op on one screen'],
    ],
  },
]

export const REQUEST_KEY = 'upnext.big.request'

export interface Request {
  /** The chosen chip's label in each group, by group name. */
  chips: Record<string, string>
  words: string
}

/** The request the chips and the words make: "About an hour, something
 *  relaxing, playing alone". Empty means "surprise me". */
export function requestText(r: Request): string {
  const parts = GROUPS.flatMap((g) => g.chips.filter(([label]) => r.chips[g.name] === label).map(([, words]) => words))
  const words = r.words.trim()
  if (words) parts.push(words)
  return upperFirst(parts.join(', '))
}
