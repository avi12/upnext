// The service's limits (`docs/VERDICTS.md`,
// "Limits" and "What the service does with them"). They aren't in the
// contract file because only the service enforces them.

// A request body, in bytes.
export const BODY_MAX = 8 * 1024;

// Per key.
export const NEW_PER_DAY = 20;
export const REPLACED_PER_DAY = 10;
export const REPORTS_PER_DAY = 20;

// Per client address, across all keys: sends and reports. Withdrawing is
// never limited, so taking a verdict down always works.
export const ADDRESS_WRITES_PER_DAY = 60;

// A text with reports from this many keys is held for a moderator, counting
// only keys with a published verdict of their own.
export const REPORTS_TO_HOLD = 3;

// Brigading: a game's new verdicts are held once its new verdicts today,
// counting the one being sent, are more than `BRIGADE_PER_DAY`, or, for a
// game with verdicts in the `BRIGADE_BASELINE_DAYS` before today, are at
// least `BRIGADE_FLOOR` and more than `BRIGADE_FACTOR` times its daily
// average over those days. The floor keeps a quiet game from being held on
// its second verdict of a day, and a game with no history (a new release,
// or one nobody had written about) has only the daily cap, since it has no
// usual rate to be ten times of.
export const BRIGADE_PER_DAY = 20;
export const BRIGADE_FACTOR = 10;
export const BRIGADE_FLOOR = 5;
export const BRIGADE_BASELINE_DAYS = 28;

// Held verdicts with no reports are counted after this many days.
export const HOLD_DAYS = 7;

// Dated snapshot files are kept this many days, today's included, and
// served with a cache lifetime of at most a day, so a withdrawn verdict is
// gone from the service's files within a week.
export const KEEP_DAYS = 6;
export const DATED_MAX_AGE = 86_400;
export const MANIFEST_MAX_AGE = 300;
