// The daily snapshot (`docs/verdicts/snapshot.md`):
// the summary installs download, the full file of every published verdict,
// and the manifest, written last. The lines are built with the same keys in
// the same order as UpNext's writer
// (`src/verdicts/format.rs`), and `summarise` is a port of its `summarise`,
// so the same verdicts give the same summary, byte for byte before gzip.

import { FORMAT, LICENCE, LIMITS, NOTICE, VOCAB } from "./contract";
import { gzip, sha256Hex } from "./util";

// One verdict as the full file publishes it: nothing about who sent it.
export interface Published {
  id: string;
  steam_appid?: number;
  gog_id?: string;
  outcome: string;
  worth: string;
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
  month: string;
}

type Counts = Record<string, number>;

export interface GameSummary {
  steam_appid?: number;
  gog_id?: string;
  n: number;
  counts: Record<string, Counts | Record<string, Counts>>;
  texts: Record<string, unknown>[];
}

export const MANIFEST = "verdicts.json";
export const summaryFile = (date: string) => `verdicts-summary-${date}.jsonl.gz`;
export const allFile = (date: string) => `verdicts-all-${date}.jsonl.gz`;

// Rust's `BTreeMap<String, _>` order: by bytes, which for these ASCII codes
// is plain code-unit order.
function byCode(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function sorted<T>(m: Map<string, T>): Record<string, T> {
  const out: Record<string, T> = {};
  for (const k of [...m.keys()].sort(byCode)) out[k] = m.get(k) as T;
  return out;
}

function add(m: Map<string, number>, v: string | undefined) {
  if (v !== undefined) m.set(v, (m.get(v) ?? 0) + 1);
}

// The key with the highest count, the lowest key on a tie, as the Rust
// `max_by_key((n, Reverse(key)))` picks it.
function most<K extends string | number>(m: Map<K, number>): K | undefined {
  let best: K | undefined;
  let bestN = -1;
  for (const [k, n] of m) {
    if (n > bestN || (n === bestN && best !== undefined && k < best)) {
      best = k;
      bestN = n;
    }
  }
  return best;
}

// Published verdicts summed up into games, as UpNext's `format::summarise`:
// games with fewer than `show_at` verdicts are left out, and each keeps its
// newest texts by month and then by id.
//
// Verdicts are one game's when they share a Steam appid, or a GOG id that a
// verdict with both ids ties to an appid; a verdict with only a GOG id nobody
// tied to an appid is a game of its own.
export function summarise(verdicts: Published[]): GameSummary[] {
  const appidOfGog = new Map<string, Map<number, number>>();
  for (const v of verdicts) {
    if (v.steam_appid !== undefined && v.gog_id !== undefined) {
      const m = appidOfGog.get(v.gog_id) ?? new Map<number, number>();
      m.set(v.steam_appid, (m.get(v.steam_appid) ?? 0) + 1);
      appidOfGog.set(v.gog_id, m);
    }
  }
  const tied = (g: string) => {
    const m = appidOfGog.get(g);
    return m ? most(m) : undefined;
  };
  // Rust orders `Key::Steam(appid)` before `Key::Gog(id)`, each by its value.
  const steam = new Map<number, Published[]>();
  const gog = new Map<string, Published[]>();
  for (const v of verdicts) {
    const appid = v.steam_appid ?? (v.gog_id !== undefined ? tied(v.gog_id) : undefined);
    if (appid !== undefined) steam.set(appid, [...(steam.get(appid) ?? []), v]);
    else if (v.gog_id !== undefined) gog.set(v.gog_id, [...(gog.get(v.gog_id) ?? []), v]);
  }
  const groups: [{ steam?: number; gog?: string }, Published[]][] = [
    ...[...steam.keys()].sort((a, b) => a - b).map((a) => [{ steam: a }, steam.get(a) as Published[]] as [{ steam: number }, Published[]]),
    ...[...gog.keys()].sort(byCode).map((g) => [{ gog: g }, gog.get(g) as Published[]] as [{ gog: string }, Published[]]),
  ];
  const out: GameSummary[] = [];
  for (const [key, list] of groups) {
    if (list.length < LIMITS.show_at) continue;
    const c = {
      outcome: new Map<string, number>(),
      worth: new Map<string, number>(),
      hours: new Map<string, number>(),
      clicked: new Map<string, number>(),
      sessions: new Map<string, number>(),
      limited_time: new Map<string, number>(),
      factors: new Map<string, number>(),
      stop_point: new Map<string, number>(),
      stopped_for: new Map<string, number>(),
    };
    const hoursByOutcome = new Map<string, Map<string, number>>();
    const gogs = new Map<string, number>();
    for (const v of list) {
      add(c.outcome, v.outcome);
      add(c.worth, v.worth);
      add(c.hours, v.hours);
      add(c.clicked, v.clicked);
      add(c.sessions, v.sessions);
      add(c.limited_time, v.limited_time);
      add(c.stop_point, v.stop_point);
      for (const f of v.factors) add(c.factors, f);
      for (const r of v.stopped_for) add(c.stopped_for, r);
      if (v.hours !== undefined) {
        const m = hoursByOutcome.get(v.outcome) ?? new Map<string, number>();
        add(m, v.hours);
        hoursByOutcome.set(v.outcome, m);
      }
      if (v.gog_id !== undefined) add(gogs, v.gog_id);
    }
    // The same keys in the same order as Rust's `GameCounts`; the optional
    // ones are left out when nobody answered.
    const counts: GameSummary["counts"] = { outcome: sorted(c.outcome), worth: sorted(c.worth) };
    for (const name of ["hours", "clicked", "sessions", "limited_time", "factors", "stop_point", "stopped_for"] as const) {
      if (c[name].size > 0) counts[name] = sorted(c[name]);
    }
    if (hoursByOutcome.size > 0) {
      counts.hours_by_outcome = sorted(new Map([...hoursByOutcome].map(([o, m]) => [o, sorted(m)])));
    }
    const texts = list
      .filter((v) => v.text !== undefined || v.stop_note !== undefined)
      .sort((a, b) => byCode(b.month, a.month) || byCode(a.id, b.id))
      .slice(0, LIMITS.texts_per_game)
      .map((v) => {
        const t: Record<string, unknown> = { id: v.id, outcome: v.outcome, worth: v.worth };
        if (v.hours !== undefined) t.hours = v.hours;
        if (v.text !== undefined) t.text = v.text;
        if (v.stop_note !== undefined) t.stop_note = v.stop_note;
        t.month = v.month;
        return t;
      });
    const game: GameSummary = { n: list.length, counts, texts };
    out.push(
      key.steam !== undefined
        ? { steam_appid: key.steam, ...withGog(most(gogs)), ...game }
        : { gog_id: key.gog as string, ...game },
    );
  }
  return out;
}

function withGog(g: string | undefined): { gog_id?: string } {
  return g === undefined ? {} : { gog_id: g };
}

// A summary line as serde writes it: `texts` is left out when empty.
function summaryLine(g: GameSummary): Record<string, unknown> {
  const { texts, ...rest } = g;
  return texts.length > 0 ? { ...rest, texts } : rest;
}

// A full-file line with Rust's `Published` field order, empty lists and
// missing answers left out.
export function publishedLine(v: Published): Record<string, unknown> {
  const out: Record<string, unknown> = { id: v.id };
  if (v.steam_appid !== undefined) out.steam_appid = v.steam_appid;
  if (v.gog_id !== undefined) out.gog_id = v.gog_id;
  out.outcome = v.outcome;
  out.worth = v.worth;
  for (const k of ["hours", "clicked", "sessions", "limited_time"] as const) if (v[k] !== undefined) out[k] = v[k];
  if (v.factors.length > 0) out.factors = v.factors;
  if (v.stop_point !== undefined) out.stop_point = v.stop_point;
  if (v.stop_note !== undefined) out.stop_note = v.stop_note;
  if (v.stopped_for.length > 0) out.stopped_for = v.stopped_for;
  if (v.text !== undefined) out.text = v.text;
  out.vocab = v.vocab;
  out.month = v.month;
  return out;
}

export interface FileRef {
  file: string;
  bytes: number;
  sha256: string;
}

export interface Snapshot {
  manifest: {
    upnext_verdicts: number;
    date: string;
    vocab: number;
    games: number;
    verdicts: number;
    licence: string;
    notice: string;
    summary: FileRef;
    all: FileRef;
  };
  summary: Uint8Array;
  all: Uint8Array;
}

// A data file: its header, with the checksum of everything after it, then
// one JSON line each.
async function writeLines(kind: "summary" | "all", date: string, vocab: number, verdicts: number, lines: unknown[]): Promise<Uint8Array> {
  const body = new TextEncoder().encode(lines.map((l) => JSON.stringify(l) + "\n").join(""));
  const header = {
    upnext_verdicts: FORMAT,
    kind,
    date,
    vocab,
    count: lines.length,
    verdicts,
    sha256: await sha256Hex(body),
    licence: LICENCE,
    notice: NOTICE,
  };
  const head = new TextEncoder().encode(JSON.stringify(header) + "\n");
  const whole = new Uint8Array(head.length + body.length);
  whole.set(head);
  whole.set(body, head.length);
  return gzip(whole);
}

// A whole snapshot from the verdicts it publishes, as UpNext's
// `format::write_snapshot`.
export async function writeSnapshot(date: string, verdicts: Published[]): Promise<Snapshot> {
  const vocab = verdicts.reduce((m, v) => Math.max(m, v.vocab), verdicts.length > 0 ? 1 : VOCAB);
  const games = summarise(verdicts);
  const counted = games.reduce((s, g) => s + g.n, 0);
  const summary = await writeLines("summary", date, vocab, counted, games.map(summaryLine));
  const all = await writeLines("all", date, vocab, verdicts.length, verdicts.map(publishedLine));
  const entry = async (file: string, data: Uint8Array): Promise<FileRef> => ({ file, bytes: data.length, sha256: await sha256Hex(data) });
  return {
    manifest: {
      upnext_verdicts: FORMAT,
      date,
      vocab,
      games: games.length,
      verdicts: verdicts.length,
      licence: LICENCE,
      notice: NOTICE,
      summary: await entry(summaryFile(date), summary),
      all: await entry(allFile(date), all),
    },
    summary,
    all,
  };
}
