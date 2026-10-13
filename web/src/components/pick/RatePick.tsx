import { ThumbsDown, ThumbsUp, type LucideIcon } from "lucide-react";
import type { MissReason, PickRating, PickVerdict } from "../../types";
import { cx } from "../../lib/format";
import { MISS_REASONS } from "../../lib/ratings";
import { CHIP, CHIP_OFF, CHIP_ON } from "../profile/words";

const THUMB =
  "btn-quiet -my-2 inline-flex items-center gap-1.5 py-2 hover:text-ink disabled:opacity-50";

/** The thumbs on a pick card: how well the pick fit the
 *  request, not a review of the game. A rating isn't an action, so the pick
 *  stays where it is. "Not a fit" asks why, in one tap you can skip, and
 *  tapping what's already chosen takes it back. */
export function RatePick({
  rating,
  busy,
  onRate,
}: {
  rating: PickRating | null | undefined;
  busy: boolean;
  onRate: (verdict: PickVerdict | null, reason?: MissReason | null) => void;
}) {
  const verdict = rating?.verdict ?? null;
  const thumb = (value: PickVerdict, label: string, Icon: LucideIcon) => {
    const on = verdict === value;
    return (
      <button
        type="button"
        aria-pressed={on}
        disabled={busy}
        onClick={() => onRate(on ? null : value)}
        className={cx(THUMB, on && "font-semibold text-accent hover:text-accent")}
      >
        <Icon
          aria-hidden="true"
          className="h-4 w-4"
          fill={on ? "currentColor" : "none"}
        />
        {label}
      </button>
    );
  };

  return (
    <div className="flex flex-col gap-2 text-[13.5px] text-dim">
      <div
        role="group"
        aria-label="How well this pick fits your request"
        className="flex flex-wrap items-center gap-x-4 gap-y-2"
      >
        {thumb("good", "Good pick", ThumbsUp)}
        {thumb("not_a_fit", "Not a fit", ThumbsDown)}
      </div>
      {verdict === "not_a_fit" && (
        <div
          role="group"
          aria-label="Why it wasn't a fit"
          className="fade-up flex flex-wrap items-center gap-1.5"
        >
          <span className="mr-1">Why? If you like:</span>
          {MISS_REASONS.map((r) => {
            const on = rating?.reason === r.value;
            return (
              <button
                key={r.value}
                type="button"
                aria-pressed={on}
                disabled={busy}
                onClick={() => onRate("not_a_fit", on ? null : r.value)}
                className={cx(CHIP, on ? CHIP_ON : CHIP_OFF, "disabled:opacity-50")}
              >
                {r.label}
              </button>
            );
          })}
        </div>
      )}
    </div>
  );
}
