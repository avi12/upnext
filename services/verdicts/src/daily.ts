// The daily run, the Worker's scheduled handler: release held verdicts
// nobody reported, delete withdrawn and removed verdicts' answers and text,
// write the day's snapshot to R2 with its manifest last, delete dated files
// older than `KEEP_DAYS`, and drop the rate limiter's counters and salts
// from before today.

import { LIMITS } from "./contract";
import type { Env } from "./env";
import * as L from "./limits";
import { MANIFEST, type Published, allFile, summaryFile, writeSnapshot } from "./snapshot";
import { addDays } from "./util";
import { checkTyped } from "./verdict";

interface Row {
  id: string;
  steam_appid: number | null;
  gog_id: string | null;
  outcome: string;
  worth: string;
  hours: string | null;
  clicked: string | null;
  sessions: string | null;
  limited_time: string | null;
  factors: string | null;
  stop_point: string | null;
  stop_note: string | null;
  stopped_for: string | null;
  text: string | null;
  text_state: string;
  vocab: number;
  updated_day: string;
}

function published(r: Row): Published {
  const shown = r.text_state === "shown";
  const v: Published = {
    id: r.id,
    outcome: r.outcome,
    worth: r.worth,
    factors: r.factors ? (JSON.parse(r.factors) as string[]) : [],
    stopped_for: r.stopped_for ? (JSON.parse(r.stopped_for) as string[]) : [],
    vocab: r.vocab,
    month: r.updated_day.slice(0, 7),
  };
  if (r.steam_appid !== null) v.steam_appid = r.steam_appid;
  if (r.gog_id !== null) v.gog_id = r.gog_id;
  for (const k of ["hours", "clicked", "sessions", "limited_time", "stop_point"] as const) {
    if (r[k] !== null) v[k] = r[k] as string;
  }
  // A text held or hidden by moderation is left out; the answers stay. So
  // is one that fails today's text checks, which can be stricter than the
  // ones it passed when it was sent: UpNext refuses a whole snapshot with
  // such a text, so one would stop every install's updates.
  if (shown && r.stop_note !== null && checkTyped(r.stop_note, LIMITS.stop_note_chars) === undefined) v.stop_note = r.stop_note;
  if (shown && r.text !== null && checkTyped(r.text, LIMITS.text_chars) === undefined) v.text = r.text;
  return v;
}

export interface DailyResult {
  date: string;
  games: number;
  verdicts: number;
  released: number;
  cleared: number;
  deletedFiles: string[];
}

export async function daily(env: Env, date: string): Promise<DailyResult> {
  const holdEnds = addDays(date, -L.HOLD_DAYS);
  const steps = await env.DB.batch([
    env.DB.prepare(
      `UPDATE verdicts SET state = 'live'
       WHERE state = 'held' AND updated_day <= ? AND NOT EXISTS (SELECT 1 FROM reports WHERE verdict_id = verdicts.id)`,
    ).bind(holdEnds),
    // Only the id, the game, the key's hash and the days are kept, so an id
    // is never used twice and a key's earlier verdicts still count against
    // its limits.
    env.DB.prepare(
      `UPDATE verdicts SET outcome = NULL, worth = NULL, hours = NULL, clicked = NULL, sessions = NULL, limited_time = NULL,
         factors = NULL, stop_point = NULL, stop_note = NULL, stopped_for = NULL, text = NULL, vocab = NULL, licence = NULL,
         consent = NULL, client_version = NULL, published_day = NULL
       WHERE state IN ('withdrawn', 'removed') AND (outcome IS NOT NULL OR published_day IS NOT NULL)`,
    ),
    env.DB.prepare("DELETE FROM reports WHERE verdict_id IN (SELECT id FROM verdicts WHERE state IN ('withdrawn', 'removed'))"),
  ]);
  const rows = await env.DB.prepare(
    `SELECT v.id, v.steam_appid, v.gog_id, v.outcome, v.worth, v.hours, v.clicked, v.sessions, v.limited_time, v.factors,
       v.stop_point, v.stop_note, v.stopped_for, v.text, v.text_state, v.vocab, v.updated_day
     FROM verdicts v JOIN keys k ON k.key_hash = v.key_hash
     WHERE v.state = 'live' AND k.blocked = 0 AND v.updated_day <= ?
     ORDER BY v.id`,
  )
    .bind(date)
    .all<Row>();
  const verdicts = rows.results.map(published);
  const snap = await writeSnapshot(date, verdicts);

  // The dated files first and the manifest last, so a reader never sees a
  // manifest that names a file not written yet.
  const dated = { cacheControl: `public, max-age=${L.DATED_MAX_AGE}`, contentType: "application/gzip" };
  await env.SNAPSHOTS.put(summaryFile(date), snap.summary, { httpMetadata: dated });
  await env.SNAPSHOTS.put(allFile(date), snap.all, { httpMetadata: dated });
  await env.SNAPSHOTS.put(MANIFEST, JSON.stringify(snap.manifest, null, 2) + "\n", {
    httpMetadata: { cacheControl: `public, max-age=${L.MANIFEST_MAX_AGE}`, contentType: "application/json" },
  });

  const oldest = addDays(date, -(L.KEEP_DAYS - 1));
  const deletedFiles: string[] = [];
  const listed = await env.SNAPSHOTS.list({ prefix: "verdicts-" });
  for (const o of listed.objects) {
    const m = /-(\d{4}-\d{2}-\d{2})\.jsonl\.gz$/.exec(o.key);
    if (m && m[1] < oldest) deletedFiles.push(o.key);
  }
  if (deletedFiles.length > 0) await env.SNAPSHOTS.delete(deletedFiles);

  await env.DB.batch([
    env.DB.prepare(
      `UPDATE verdicts SET published_day = ?
       WHERE state = 'live' AND updated_day <= ? AND key_hash IN (SELECT key_hash FROM keys WHERE blocked = 0)`,
    ).bind(date, date),
    env.DB.prepare("DELETE FROM counters WHERE day < ?").bind(date),
    env.DB.prepare("DELETE FROM salts WHERE day < ?").bind(date),
  ]);
  return {
    date,
    games: snap.manifest.games,
    verdicts: snap.manifest.verdicts,
    released: steps[0].meta.changes ?? 0,
    cleared: steps[1].meta.changes ?? 0,
    deletedFiles,
  };
}
