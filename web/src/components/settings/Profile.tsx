import { Link } from 'react-router'
import { useQuery } from '@tanstack/react-query'
import { api } from '../../api'
import { Row, Section } from './parts'
import { plural } from '../../lib/format'

/** A way into the taste profile, which has a page of its own. */
export function ProfileSection() {
  const sources = useQuery({ queryKey: ['taste-sources'], queryFn: api.tasteSources, staleTime: 30_000 })
  const a = sources.data?.answers
  const said = a ? a.favourites + a.likes + a.dislikes + a.hard_nos + a.habits : 0
  return (
    <Section id="profile" title="Profile" desc="What UpNext knows about you as a player">
      <Row
        name="Your taste profile"
        desc={
          a
            ? said
              ? `${plural(a.favourites, 'favourite')}, ${plural(a.likes + a.dislikes + a.hard_nos, 'like or dislike', 'likes and dislikes')}, ${plural(a.habits, 'habit')}, and the patterns UpNext counted`
              : 'Favourites, likes and hard nos, and the patterns UpNext counted from how you play'
            : undefined
        }
        right={
          <Link to="/profile" className="btn-outline h-10">
            Open it
          </Link>
        }
      />
    </Section>
  )
}
