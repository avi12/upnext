// The contract the service shares with UpNext, read from the file UpNext
// writes (`docs/verdicts-contract.json`, kept current by a Rust test), so the
// answers, limits and text checks here are the app's own and are never
// retyped. Where something isn't in that file, it is written here with a
// test that holds it to the app's fixtures or to `docs/VERDICTS.md`.

import contract from "../../../docs/verdicts-contract.json";

export interface Choice {
  code: string;
  label: string;
}

export interface FieldSpec {
  name: string;
  question: string;
  required: boolean;
  many?: boolean;
  text?: boolean;
  with?: string;
  only_for?: string[];
  max_chars?: number;
  choices?: Choice[];
}

export interface TextCheckSpec {
  code: string;
  why: string;
  pattern: string;
  flags: string;
}

export const CONTRACT = contract;
export const VOCAB: number = contract.vocab;
export const FORMAT: number = contract.format;
export const LICENCE: string = contract.licence;
export const CONSENT_VERSION: number = contract.consent.version;
export const KEY_PREFIX: string = contract.key_prefix;
export const LIMITS = contract.limits;
export const FIELDS: FieldSpec[] = contract.fields;
export const REPORT_REASONS: Choice[] = contract.report_reasons;

// The oldest consent version the service accepts. UpNext's `OLDEST_CONSENT`
// (`src/verdicts/verdict.rs`) isn't in the contract file; raise both together.
export const OLDEST_CONSENT = 1;

// What every header and the manifest say about the data, as UpNext's
// `format::NOTICE`. A test holds it to the fixture snapshot's manifest.
export const NOTICE =
  "UpNext backlog verdicts: players' answers about how games fit a backlog and limited time, " +
  "dedicated to the public domain under CC0 1.0 (https://creativecommons.org/publicdomain/zero/1.0/). " +
  "They carry no names, accounts or libraries, and nothing in them links one person's verdicts together.";

// The life reasons for stopping a game, which are about the person rather than the game
// and are never sent. They aren't in the contract file because no verdict
// may carry them; they're named here only to say why one is refused.
export const LIFE_REASONS = ["busy", "other_game", "mood", "elsewhere"];

function field(name: string): FieldSpec {
  const f = FIELDS.find((x) => x.name === name);
  if (!f) throw new Error(`the contract has no field ${name}`);
  return f;
}

function codes(name: string): Set<string> {
  return new Set((field(name).choices ?? []).map((c) => c.code));
}

// The questions with one answer each, in the order UpNext checks them.
export const SINGLE_ANSWERS = ["outcome", "worth", "hours", "clicked", "sessions", "limited_time", "stop_point"] as const;
export type SingleAnswer = (typeof SINGLE_ANSWERS)[number];

export const ANSWERS: Record<SingleAnswer, Set<string>> = Object.fromEntries(
  SINGLE_ANSWERS.map((n) => [n, codes(n)]),
) as Record<SingleAnswer, Set<string>>;
export const REQUIRED: Set<string> = new Set(FIELDS.filter((f) => f.required).map((f) => f.name));
export const FACTORS: Set<string> = codes("factors");
export const STOP_REASONS: Set<string> = codes("stopped_for");
export const STOPPED_OUTCOMES: Set<string> = new Set(field("stopped_for").only_for ?? []);
export const REASON_CODES: Set<string> = new Set(REPORT_REASONS.map((r) => r.code));

export interface TextCheck {
  code: string;
  why: string;
  // The contract's string, kept as it is: `RegExp.source` escapes slashes.
  pattern: string;
  re: RegExp;
}

// JavaScript's `i` alone folds case only within ASCII, while UpNext's
// `(?i)` folds by Unicode, so the long s (U+017F) matches `s` and the
// Kelvin sign (U+212A) matches `k` there. Without `u` here, a text like
// `me@host.ſſ` would pass the service and then make every install refuse
// the whole snapshot.
function unicode(flags: string): string {
  return flags.includes("u") ? flags : `${flags}u`;
}

// The text checks with the contract's exact pattern strings and flags, plus
// `u`. The first that matches is the one named.
export const TEXT_CHECKS: TextCheck[] = (contract.text_checks as TextCheckSpec[]).map((c) => ({
  code: c.code,
  why: c.why,
  pattern: c.pattern,
  re: new RegExp(c.pattern, unicode(c.flags)),
}));

// The characters that show as nothing and change no words (the soft hyphen,
// zero-width spaces and joiners, direction marks, the byte order mark),
// taken out before the text checks run so they can't hide a link. The
// contract gives it the `g` flag, for replacing every one.
export const TEXT_IGNORED = new RegExp(contract.text_ignored.pattern, unicode(contract.text_ignored.flags));
