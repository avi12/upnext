import { useRef } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { api, ApiError } from "../../api";
import type {
  FeedbackAction,
  MissReason,
  OverrideStatus,
  OwnedPick,
  PickBatch,
  PickVerdict,
  Source,
} from "../../types";
import type { Reasons } from "../StatusMenu";
import { useToast } from "../useToast";
import { showLineup } from "./useGameActions";

function message(e: unknown): string {
  return e instanceof ApiError || e instanceof Error ? e.message : String(e);
}

export interface CardAction {
  pick: OwnedPick;
  action: FeedbackAction;
  /** How it went, for "I've started it…". */
  text?: OverrideStatus;
  reasons?: Reasons;
}

/** A lineup's cards: Play, Not tonight, Never recommend this and
 *  "I've started it…" answer at once with the lineup as it stands after;
 *  "It's too ___" waits for the AI. A slot left empty asks for new spares.
 *  Every AI failure is a toast with the reason. The thumbs save a
 *  rating and change nothing else. `input` says new spares are asked for
 *  on a controller, as big screen's first request was. */
export function useLineup(batch: PickBatch | null, sources?: Source[], input?: "controller") {
  const qc = useQueryClient();
  const toast = useToast();
  const batchId = batch?.batch_id ?? "";
  // One request for new spares at a time, however many slots empty at once.
  const asking = useRef(false);

  const show = (b: PickBatch) => showLineup(qc, b);

  const more = useMutation({
    mutationFn: (id: string) => api.pickMore(id, sources, input),
    onMutate: () => {
      asking.current = true;
    },
    onSettled: () => {
      asking.current = false;
    },
    onSuccess: show,
    onError: (e) => toast.error("Couldn't find another pick", message(e)),
  });

  /** Another action changed the lineup first (409): show it as it stands
   *  now, so the cards match what the server holds. */
  const reload = (e: unknown) => {
    if (!(e instanceof ApiError) || e.status !== 409) return;
    api.history(20).then(
      (all) => {
        const now = all.find((b) => b.batch_id === batchId);
        if (now) show(now);
      },
      () => {},
    );
  };

  /** Show the lineup, and fill any empty slot from new spares. */
  const after = (b: PickBatch) => {
    show(b);
    if (b.picks.length < b.shown && !asking.current) more.mutate(b.batch_id);
  };

  const act = useMutation({
    mutationFn: ({ pick, action, text, reasons }: CardAction) =>
      api.pickFeedback(batchId, { title_id: pick.id, action, text, reasons }),
    onSuccess: (b, { pick, action }) => {
      after(b);
      if (action === "chosen") return;
      qc.invalidateQueries({ queryKey: ["games"] });
      qc.invalidateQueries({ queryKey: ["stats"] });
      qc.invalidateQueries({ queryKey: ["collections"] });
      qc.invalidateQueries({ queryKey: ["game", pick.id] });
      if (action === "not_for_me") {
        toast.success(`${pick.game.name} won't come up again`, "Your Library lists it under Not for me, if you change your mind.", {
          label: "Undo",
          onClick: () => {
            api.setOverride(pick.id, null).then(
              () => qc.invalidateQueries({ queryKey: ["game", pick.id] }),
              (e) => toast.error("Couldn't undo that", message(e)),
            );
          },
        });
        if (b.mode === "new_to_you") offerHardNo(pick);
      }
    },
    onError: (e, { action }) => {
      reload(e);
      toast.error(
        action === "chosen"
          ? "Couldn't record that you played it"
          : "Couldn't change the lineup",
        message(e),
      );
    },
  });

  /** New to you: turning a pick down offers to rule out what it
   *  was a way into, so picks stop trying it. */
  const offerHardNo = (pick: OwnedPick) => {
    api.newToYou(pick.id).then(({ ways }) => {
      const way = ways[0];
      if (!way) return;
      toast.info(`Not into ${way.phrase} at all?`, undefined, {
        label: `Make ${way.phrase} a hard no`,
        onClick: () => {
          api.tasteState({ kind: "hard_no", about: way.about }).then(
            () => {
              qc.invalidateQueries({ queryKey: ["taste"] });
              toast.success(`No more ${way.phrase}`, "It's a hard no on your profile now.");
            },
            (e) => toast.error("Couldn't add that hard no", message(e)),
          );
        },
      });
    }, () => {});
  };

  const too = useMutation({
    mutationFn: ({ pick, word }: { pick: OwnedPick; word: string }) =>
      api.pickToo(batchId, pick.id, word),
    onSuccess: (out, { pick, word }) => {
      after(out.batch);
      qc.invalidateQueries({ queryKey: ["game", pick.id] });
      if (out.ai_error) {
        const into = out.swaps[0]?.into;
        const name = out.batch.picks.find((p) => p.id === into)?.game?.name;
        toast.error(
          "The AI couldn't check the spares",
          name
            ? `${out.ai_error}. UpNext swapped in ${name} on its own as one that isn't too ${word}, without the AI's check.`
            : `${out.ai_error}.`,
        );
      }
    },
    onError: (e) => {
      reload(e);
      toast.error("Couldn't swap that pick", message(e));
    },
  });

  /** How well a pick fit. Not an action: the lineup stays as it
   *  is, and the answer only carries the new rating. */
  const rate = useMutation({
    mutationFn: ({
      pick,
      verdict,
      reason,
    }: {
      pick: OwnedPick;
      verdict: PickVerdict | null;
      reason?: MissReason | null;
    }) => api.ratePick(batchId, { title_id: pick.id, verdict, reason }),
    onSuccess: (b, { pick }) => {
      show(b);
      qc.invalidateQueries({ queryKey: ["game", pick.id] });
      qc.invalidateQueries({ queryKey: ["taste-sources"] });
    },
    onError: (e) => toast.error("Couldn't save that rating", message(e)),
  });

  /** A wishlisted pick swaps for the owned game it came with. */
  const swap = useMutation({
    mutationFn: (titleId: number) => api.pickSwitch(batchId, titleId),
    onSuccess: after,
    onError: (e) => {
      reload(e);
      toast.error("Couldn't switch that pick", message(e));
    },
  });

  return { act, too, more, swap, rate };
}
