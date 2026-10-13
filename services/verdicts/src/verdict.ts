// One verdict as it is sent, read strictly and checked the way UpNext's
// `Verdict::check` checks it (`src/verdicts/verdict.rs`), with the same
// problems in the same order and the same words, so the form and the service
// say the same thing about the same mistake.

import {
  ANSWERS,
  CONSENT_VERSION,
  FACTORS,
  LICENCE,
  LIFE_REASONS,
  LIMITS,
  OLDEST_CONSENT,
  REQUIRED,
  SINGLE_ANSWERS,
  STOPPED_OUTCOMES,
  STOP_REASONS,
  TEXT_CHECKS,
  TEXT_IGNORED,
  type TextCheck,
  VOCAB,
} from "./contract";

export interface Verdict {
  steam_appid?: number;
  gog_id?: string;
  outcome?: string;
  worth?: string;
  hours?: string;
  clicked?: string;
  sessions?: string;
  limited_time?: string;
  factors: string[];
  stop_point?: string;
  stop_note?: string;
  stopped_for: string[];
  text?: string;
  vocab: number;
  licence: string;
  consent: number;
  client: string;
}

// A verdict with nothing wrong with it: its two required answers are there.
export type Accepted = Verdict & { outcome: string; worth: string };

export interface Problem {
  field: string;
  why: string;
}

// Every field a verdict may have. Anything else refuses it, as UpNext's
// `deny_unknown_fields` does.
const STRINGS = ["gog_id", ...SINGLE_ANSWERS, "stop_note", "text", "licence", "client"];
const LISTS = ["factors", "stopped_for"];
const INTEGERS = ["steam_appid", "vocab", "consent"];
const KNOWN = new Set([...STRINGS, ...LISTS, ...INTEGERS]);

// Characters as Unicode scalar values, as Rust's `chars().count()`.
export function chars(s: string): number {
  return [...s].length;
}

// The first check typed text fails, if any, once the characters that show
// as nothing (`TEXT_IGNORED`) are taken out, as UpNext's `text_refusal`.
export function textRefusal(text: string): TextCheck | undefined {
  const seen = text.replace(TEXT_IGNORED, "");
  return TEXT_CHECKS.find((c) => c.re.test(seen));
}

// Check typed text: not blank, at most `max` characters, and none of the
// text checks. Says which part is refused rather than dropping it.
export function checkTyped(text: string, max: number): string | undefined {
  if (text.trim() === "") return "is empty; leave it out instead";
  const n = chars(text);
  if (n > max) return `is ${n} characters; at most ${max}`;
  const c = textRefusal(text);
  return c ? `${c.why}; take it out to send` : undefined;
}

export function isGogId(s: string): boolean {
  return s.length > 0 && s.length <= 20 && /^[0-9]+$/.test(s);
}

function hasRepeats(list: string[]): boolean {
  return new Set(list).size !== list.length;
}

// Rust's `{:?}` of a string, which the messages quote answers with.
function quoted(s: string): string {
  return JSON.stringify(s);
}

// Read a body as a verdict. A body that isn't one (an unknown field, a value
// of the wrong type) comes back as problems with no verdict; one that reads
// comes back with everything `check` finds wrong with it.
export function readVerdict(body: unknown): { verdict?: Verdict; problems: Problem[] } {
  if (typeof body !== "object" || body === null || Array.isArray(body)) {
    return { problems: [{ field: "body", why: "isn't a verdict: a JSON object" }] };
  }
  const raw = body as Record<string, unknown>;
  const problems: Problem[] = [];
  for (const k of Object.keys(raw)) {
    if (!KNOWN.has(k)) problems.push({ field: k, why: "isn't a field of a verdict" });
  }
  const out: Record<string, unknown> = { factors: [], stopped_for: [] };
  for (const [k, v] of Object.entries(raw)) {
    // `null` reads as left out, as Rust's `Option` does.
    if (!KNOWN.has(k) || v === null || v === undefined) continue;
    if (STRINGS.includes(k)) {
      if (typeof v !== "string") problems.push({ field: k, why: "isn't a string" });
      else out[k] = v;
    } else if (LISTS.includes(k)) {
      if (!Array.isArray(v) || v.some((x) => typeof x !== "string")) problems.push({ field: k, why: "isn't a list of strings" });
      else out[k] = v;
    } else if (k === "steam_appid") {
      if (typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 0xffffffff) problems.push({ field: k, why: "isn't a Steam appid" });
      else out[k] = v;
    } else if (typeof v !== "number" || !Number.isInteger(v)) {
      problems.push({ field: k, why: "isn't a whole number" });
    } else {
      out[k] = v;
    }
  }
  for (const k of ["vocab", "licence", "consent", "client"]) {
    if (raw[k] === undefined || raw[k] === null) problems.push({ field: k, why: "is required" });
  }
  if (problems.length > 0) return { problems };
  const verdict = out as unknown as Verdict;
  return { verdict, problems: check(verdict) };
}

// Everything wrong with a verdict, the answers first; empty when it can be
// accepted. The same checks, in the same order, as `Verdict::check`.
export function check(v: Verdict): Problem[] {
  const out: Problem[] = [];
  const bad = (field: string, why: string) => out.push({ field, why });
  if (v.steam_appid === 0) bad("steam_appid", "isn't a Steam appid");
  if (v.gog_id !== undefined && !isGogId(v.gog_id)) bad("gog_id", "isn't a GOG id: digits only");
  if (v.steam_appid === undefined && v.gog_id === undefined) bad("game", "a verdict needs the game's Steam appid or GOG id");
  for (const name of SINGLE_ANSWERS) {
    const value = v[name];
    if (value === undefined) {
      if (REQUIRED.has(name)) bad(name, "is required");
    } else if (!ANSWERS[name].has(value)) {
      bad(name, `${quoted(value)} isn't one of its answers`);
    }
  }
  if (v.factors.length > 0 && v.limited_time === undefined) bad("factors", "go with an answer to whether it respects limited time");
  if (v.factors.length > LIMITS.factors) bad("factors", `at most ${LIMITS.factors}`);
  const unknownFactor = v.factors.find((f) => !FACTORS.has(f));
  if (unknownFactor !== undefined) bad("factors", `${quoted(unknownFactor)} isn't one of them`);
  if (hasRepeats(v.factors)) bad("factors", "each at most once");
  if (v.stop_note !== undefined) {
    if (v.stop_point === undefined) bad("stop_note", "goes with an answer to when it's fine to stop");
    const why = checkTyped(v.stop_note, LIMITS.stop_note_chars);
    if (why) bad("stop_note", why);
  }
  if (v.stopped_for.length > 0 && v.outcome !== undefined && !STOPPED_OUTCOMES.has(v.outcome)) {
    bad("stopped_for", "is only for games shelved, stopped or bounced");
  }
  const unknownReason = v.stopped_for.find((r) => !STOP_REASONS.has(r));
  if (unknownReason !== undefined) {
    bad(
      "stopped_for",
      LIFE_REASONS.includes(unknownReason)
        ? `${quoted(unknownReason)} is about you, not the game, and is never sent`
        : `${quoted(unknownReason)} isn't one of them`,
    );
  }
  if (hasRepeats(v.stopped_for)) bad("stopped_for", "each at most once");
  if (v.text !== undefined) {
    const why = checkTyped(v.text, LIMITS.text_chars);
    if (why) bad("text", why);
  }
  if (v.vocab < 1 || v.vocab > VOCAB) bad("vocab", `${v.vocab} isn't a vocabulary this build knows (1 to ${VOCAB})`);
  if (v.licence !== LICENCE) bad("licence", `verdicts are published under ${LICENCE} only`);
  if (v.consent < OLDEST_CONSENT || v.consent > CONSENT_VERSION) {
    bad("consent", `consent version ${v.consent} isn't accepted (${OLDEST_CONSENT} to ${CONSENT_VERSION})`);
  }
  if (v.client === "" || v.client.length > LIMITS.client_chars || !/^[A-Za-z0-9.+-]+$/.test(v.client)) {
    bad("client", "isn't an UpNext version");
  }
  return out;
}
