/** Your rating as the Library shows it on a cover or in the table: "★ 4",
 *  read out as "your rating: 4 of 5". */
export function RatingMark({ stars }: { stars: number }) {
  return (
    <span className="text-accent">
      <span aria-hidden="true">★ {stars}</span>
      <span className="sr-only">your rating: {stars} of 5</span>
    </span>
  )
}
