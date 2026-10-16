import { useId, useState, type CSSProperties } from "react";
import type {
  FeedbackAction,
  GameKind,
  GameView,
  MissReason,
  OverrideStatus,
  OwnedPick,
  PickVerdict,
} from "../../types";
import { cx, linuxClass } from "../../lib/format";
import { overrideMeta } from "../../lib/statuses";
import { SentenceMenu } from "../SentenceMenu";
import { WhyStep, type Reasons } from "../StatusMenu";
import {
  BookmarkIcon,
  CurateIcon,
  DiceIcon,
  PlayIcon,
  TrailerIcon,
} from "./icons";
import { historyLine, linuxShown, metaLine } from "./text";
import { playUrl } from "../library/sheetText";
import { api } from "../../api";
import { useGameActions } from "./useGameActions";
import { RatePick } from "./RatePick";
import { PickArt } from "./PickArt";

/** How "I've started it…" ends, for a game with an ending and an endless one. */
const STARTED: Record<GameKind, { value: OverrideStatus; label: string }[]> = {
  ending: [
    { value: "done", label: "…and beat it" },
    { value: "shelved", label: "…and shelved it" },
    { value: "abandoned", label: "…and abandoned it" },
    { value: "hidden", label: "…and it's not for me" },
  ],
  endless: [
    { value: "still_playing", label: "…and still play it now and then" },
    { value: "burnt_out", label: "…and burnt out on it" },
    { value: "hidden", label: "…and it's not for me" },
  ],
};

const ACTION =
  "btn-quiet -my-2 py-2 underline decoration-underline underline-offset-4 hover:text-ink disabled:opacity-50";

/** One pick in the lineup: number, compare tick, art, why, and what to do
 *  next. Its actions say what you decided: Not tonight, "It's too ___",
 *  Never recommend this, and, when UpNext doesn't know you've played it,
 *  "I've started it…". Its thumbs say how well it fit.
 *
 *  A game drawn with the dice is one card on its own, so it has no
 *  number or compare tick, and no thumbs, since chance didn't try to fit
 *  anything. Another and "Curate around this" take Not tonight's and
 *  "It's too ___"'s place. */
export function LineupColumn({
  pick,
  number,
  compared,
  onCompare,
  onOpen,
  showLinux,
  busy,
  sifting,
  onAct,
  onToo,
  rating = false,
  onRate,
  dice,
  className,
  style,
}: {
  pick: OwnedPick;
  number: number;
  compared: boolean;
  onCompare: (on: boolean) => void;
  onOpen: (g: GameView) => void;
  showLinux: boolean;
  /** An action on this card is in flight. */
  busy: boolean;
  /** The word, while "It's too ___" on this card is with the AI. */
  sifting: string | null;
  onAct: (
    action: FeedbackAction,
    text?: OverrideStatus,
    reasons?: Reasons,
  ) => void;
  onToo: (word: string) => void;
  /** A rating of this pick is being saved. */
  rating?: boolean;
  /** Rate how well it fit, or take the rating back (null). */
  onRate?: (verdict: PickVerdict | null, reason?: MissReason | null) => void;
  /** Drawn with the dice: draw another, or curate games like this one. */
  dice?: { onAnother: () => void; onCurateAround: () => void };
  className?: string;
  style?: CSSProperties;
}) {
  const g = pick.game;
  const { later } = useGameActions();
  const linux = showLinux ? linuxShown(g) : null;
  const saved = g.saved_at != null;
  const titleId = `pick-${g.id}-title`;
  const tooId = useId();
  const [word, setWord] = useState("");
  // How it went, while its "why did you stop?" step is showing.
  const [asking, setAsking] = useState<OverrideStatus | null>(null);
  const history = historyLine(g);
  // No store reports playing it: only you can say you've started it.
  const unknownPlay = !g.playtime_known || g.playtime_minutes === 0;
  const locked = busy || sifting != null;

  const sayToo = () => {
    const w = word.trim();
    if (!w || locked) return;
    onToo(w);
    setWord("");
  };
  const started = (value: OverrideStatus) => {
    if (overrideMeta(value)?.asksWhy) setAsking(value);
    else onAct("started", value);
  };

  return (
    <article
      aria-labelledby={titleId}
      aria-busy={sifting != null}
      className={cx("fade-up flex min-w-0 flex-col gap-3", className)}
      style={style}
    >
      {!dice && (
        <div className="flex items-center justify-between">
          <div
            className="font-display text-[44px] leading-[0.9] font-extrabold text-accent"
            aria-hidden="true"
          >
            {number}
          </div>
          <label
            className={cx(
              "flex min-h-11 cursor-pointer items-center gap-2 text-sm transition-colors",
              compared ? "text-ink" : "text-dim hover:text-muted",
            )}
          >
            <input
              type="checkbox"
              checked={compared}
              onChange={(e) => onCompare(e.target.checked)}
              className="m-0 h-[18px] w-[18px] cursor-pointer accent-accent"
              aria-describedby={titleId}
            />
            Compare
          </label>
        </div>
      )}

      <PickArt
        game={g}
        onOpen={() => onOpen(g)}
        className={cx(
          "w-4/5 transition-opacity",
          sifting != null && "sifting rounded-[10px] opacity-70",
          g.override && "opacity-50",
        )}
      />

      <h2
        id={titleId}
        className="mt-1.5 font-display text-[25px] leading-[1.15] font-bold tracking-[-0.015em]"
      >
        <button
          type="button"
          onClick={() => onOpen(g)}
          className="text-left hover:underline hover:decoration-underline hover:underline-offset-4"
        >
          {g.name}
        </button>
      </h2>

      <div className="flex flex-col gap-1">
        {linux && (
          <span className={cx("text-[13.5px]", linuxClass(g.linux))}>
            ● {linux} on Linux
          </span>
        )}
        <div className="font-mono text-[11px] leading-[1.6] text-dim">
          {metaLine(g)}
        </div>
        {history && <div className="text-[13.5px] text-muted">{history}</div>}
      </div>

      {sifting != null && (
        <p
          role="status"
          className="m-0 flex items-center gap-2 text-[14.5px] text-accent"
        >
          <CurateIcon busy className="h-4 w-4 shrink-0" />
          Looking through the spares for one that isn't too {sifting}…
        </p>
      )}
      <p
        className={cx(
          "mt-0.5 text-base leading-[1.6] transition-opacity",
          sifting != null && "opacity-50",
        )}
      >
        {pick.reason}
      </p>
      {pick.backlog && (
        <p className="text-[14.5px] leading-[1.6] text-muted">
          {pick.backlog}
        </p>
      )}
      {g.vibes && (
        <p className="text-[14.5px] leading-[1.6] text-muted">
          Vibes: {g.vibes}
        </p>
      )}
      {g.digest && (
        <p className="text-[14.5px] leading-[1.6] text-muted">
          People say: {g.digest}
        </p>
      )}

      <div className="mt-1.5 flex flex-wrap items-center gap-x-5 gap-y-3 text-[14.5px]">
        {playUrl(g) && (
          <a
            href={playUrl(g)!}
            className="btn-ink"
            onClick={() => {
              onAct("chosen");
              void api.diaryLaunch(g.id);
            }}
          >
            <PlayIcon />
            Play
          </a>
        )}
        {g.trailer_url && (
          <a
            href={g.trailer_url}
            target="_blank"
            rel="noreferrer"
            className="inline-flex min-h-10 items-center gap-[7px] font-semibold text-ink hover:text-accent"
          >
            <TrailerIcon />
            Trailer
          </a>
        )}
        <button
          type="button"
          onClick={() => later.mutate({ id: g.id, save: !saved })}
          disabled={later.isPending}
          aria-pressed={saved}
          className="inline-flex min-h-10 items-center gap-[7px] font-semibold text-ink hover:text-accent disabled:opacity-60"
          title={saved ? "Saved to Play later. Click to remove." : undefined}
        >
          <BookmarkIcon
            filled={saved}
            className={cx("h-3.5 w-3.5", saved && "text-accent")}
          />
          {saved ? "Saved" : "Save for later"}
        </button>
      </div>

      {dice && (
        <div className="flex flex-wrap items-center gap-3 text-[14.5px]">
          <button
            type="button"
            className="btn-outline"
            disabled={locked}
            onClick={dice.onAnother}
            title="Roll the dice again (R)"
          >
            <DiceIcon className="h-4 w-4 text-muted" />
            Another
          </button>
          <button
            type="button"
            className="btn-outline"
            disabled={locked}
            onClick={dice.onCurateAround}
            title={`Curate games like ${g.name}`}
          >
            <CurateIcon className="h-4 w-4 text-muted" />
            Curate around this
          </button>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-x-[18px] gap-y-3 text-[13.5px] text-dim">
        {!dice && (
          <button
            type="button"
            className={ACTION}
            disabled={locked}
            onClick={() => onAct("not_tonight")}
          >
            Not tonight
          </button>
        )}
        {/* A field that looks like one, with an example rather than a value,
            and Swap from the start. */}
        {!dice && (
          <form
            className="inline-flex items-center gap-1.5"
            onSubmit={(e) => {
              e.preventDefault();
              sayToo();
            }}
          >
            <label htmlFor={tooId}>It's too</label>
            <input
              id={tooId}
              value={word}
              maxLength={40}
              disabled={locked}
              onChange={(e) => setWord(e.target.value)}
              onKeyDown={(e) => e.key === "Escape" && setWord("")}
              placeholder="long, slow, grindy…"
              enterKeyHint="go"
              autoComplete="off"
              className="h-8 w-[9.5rem] cursor-text rounded-md border border-line-strong bg-raised px-2 text-[13.5px] text-ink outline-none placeholder:text-dim focus:border-accent disabled:opacity-50"
            />
            <button
              type="submit"
              className="btn-quiet -my-2 py-2 font-semibold text-accent hover:underline disabled:text-dim disabled:no-underline"
              disabled={locked || !word.trim()}
              title={word.trim() ? undefined : "Type a word first"}
            >
              Swap
            </button>
          </form>
        )}
        <button
          type="button"
          className={ACTION}
          disabled={locked}
          onClick={() => onAct("not_for_me")}
        >
          Never recommend this
        </button>
        {unknownPlay && (
          <span className={cx(locked && "pointer-events-none opacity-50")}>
            <SentenceMenu
              title="I've started it"
              label="I've started it…"
              align="left"
              options={STARTED[g.kind ?? "ending"].map((o) => ({
                key: o.value,
                label: o.label,
                checked: false,
                onSelect: () => started(o.value),
              }))}
            />
          </span>
        )}
      </div>
      {asking && (
        <WhyStep
          status={asking}
          className="fade-up w-full max-w-[340px] rounded-xl border border-line-strong bg-sheet p-1.5"
          style={{}}
          onBack={() => setAsking(null)}
          onCancel={() => setAsking(null)}
          onDone={(reasons) => {
            onAct("started", asking, reasons);
            setAsking(null);
          }}
        />
      )}
      {onRate && !dice && (
        <RatePick rating={pick.rating} busy={rating} onRate={onRate} />
      )}
    </article>
  );
}

/** A slot with no pick: new spares on the way, or a way to ask again. */
export function EmptySlot({
  finding,
  failed,
  onFind,
  className,
}: {
  finding: boolean;
  failed: boolean;
  onFind: () => void;
  className?: string;
}) {
  return (
    <div
      className={cx("flex min-w-0 flex-col gap-3", className)}
      role="status"
      aria-live="polite"
    >
      <div className="h-10 w-7 rounded-md bg-raised" aria-hidden="true" />
      <div
        className={cx(
          "aspect-[460/215] w-4/5 rounded-[10px]",
          finding ? "shimmer" : "bg-raised",
        )}
        aria-hidden="true"
      />
      {finding ? (
        <p className="m-0 flex items-center gap-2 text-[15px] text-muted">
          <CurateIcon busy className="h-4 w-4 shrink-0" />
          Finding another…
        </p>
      ) : (
        <div className="flex flex-col items-start gap-2">
          <p className="m-0 text-[15px] text-muted">
            {failed ? "Couldn't find another pick." : "The spares ran out."}
          </p>
          <button type="button" className="btn-outline h-10" onClick={onFind}>
            {failed ? "Try again" : "Find another"}
          </button>
        </div>
      )}
    </div>
  );
}
