// The endpoint (`docs/VERDICTS.md`, "The endpoint"): send or replace a
// verdict, withdraw one or all, list a key's own, and report a text. Every
// request carries a player's key; the service keeps only its SHA-256.

import { KEY_PREFIX, REASON_CODES } from "./contract";
import type { Env } from "./env";
import * as L from "./limits";
import { addDays, clientNetwork, hmacHex, isVerdictId, newVerdictId, secondsToMidnight, sha256Hex, today } from "./util";
import { type Accepted, type Problem, readVerdict } from "./verdict";

export function json(status: number, body: unknown, headers: Record<string, string> = {}): Response {
  return new Response(status === 204 ? null : JSON.stringify(body), {
    status,
    headers: { "content-type": "application/json", "cache-control": "no-store", ...headers },
  });
}

export function failure(status: number, error: string, extra: Record<string, unknown> = {}, headers: Record<string, string> = {}): Response {
  return json(status, { error, ...extra }, headers);
}

function tooMany(why: string, retryAfter: number): Response {
  return failure(429, why, {}, { "retry-after": String(retryAfter) });
}

const KEY_SHAPE = new RegExp(`^${KEY_PREFIX}[0-9a-f]{64}$`);

// The key's hash, or the response refusing the request.
async function authenticate(req: Request): Promise<string | Response> {
  const m = /^Bearer (.+)$/.exec(req.headers.get("authorization") ?? "");
  if (!m || !KEY_SHAPE.test(m[1])) {
    return failure(401, `send the player's key as "Authorization: Bearer ${KEY_PREFIX}…"`);
  }
  return sha256Hex(m[1]);
}

// A JSON body of at most `BODY_MAX` bytes, or the response refusing it.
async function body(req: Request): Promise<unknown | Response> {
  const type = (req.headers.get("content-type") ?? "").split(";")[0].trim().toLowerCase();
  if (type !== "application/json") return failure(415, "send the body as application/json");
  const declared = Number(req.headers.get("content-length") ?? "0");
  if (declared > L.BODY_MAX) return failure(413, `the body is over ${L.BODY_MAX} bytes`);
  const raw = await readAtMost(req, L.BODY_MAX);
  if (!raw) return failure(413, `the body is over ${L.BODY_MAX} bytes`);
  try {
    return JSON.parse(new TextDecoder("utf-8", { fatal: true, ignoreBOM: false }).decode(raw));
  } catch {
    return failure(400, "the body isn't JSON");
  }
}

// The body's bytes, or null once it's past `max`. A body sent without a
// length (chunked) is read only that far, never whole.
async function readAtMost(req: Request, max: number): Promise<Uint8Array | null> {
  if (!req.body) return new Uint8Array(0);
  const reader = req.body.getReader();
  const chunks: Uint8Array[] = [];
  let length = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    length += value.byteLength;
    if (length > max) {
      await reader.cancel();
      return null;
    }
    chunks.push(value);
  }
  const raw = new Uint8Array(length);
  let at = 0;
  for (const c of chunks) {
    raw.set(c, at);
    at += c.byteLength;
  }
  return raw;
}

// Whether a moderator blocked the key. The key's row is written only with
// its first verdict, so a request that's refused leaves nothing behind.
async function blocked(env: Env, keyHash: string): Promise<boolean> {
  const row = await env.DB.prepare("SELECT blocked FROM keys WHERE key_hash = ?").bind(keyHash).first<{ blocked: number }>();
  return (row?.blocked ?? 0) !== 0;
}

// The client's counter name: an HMAC of its address, or of its IPv6 /64
// (`clientNetwork`), under a random salt made for the day, so the address
// itself is never stored and a counter can't be traced back once the day's
// salt is deleted.
async function addressName(env: Env, req: Request, day: string): Promise<string> {
  const fresh = [...crypto.getRandomValues(new Uint8Array(32))].map((b) => b.toString(16).padStart(2, "0")).join("");
  await env.DB.prepare("INSERT INTO salts (day, salt) VALUES (?, ?) ON CONFLICT DO NOTHING").bind(day, fresh).run();
  const row = (await env.DB.prepare("SELECT salt FROM salts WHERE day = ?").bind(day).first()) as { salt: string };
  const address = clientNetwork(req.headers.get("cf-connecting-ip") ?? "unknown");
  return `addr:${await hmacHex(row.salt, address)}`;
}

async function counts(env: Env, day: string, names: string[]): Promise<Record<string, number>> {
  const marks = names.map(() => "?").join(", ");
  const rows = await env.DB.prepare(`SELECT name, n FROM counters WHERE day = ? AND name IN (${marks})`)
    .bind(day, ...names)
    .all<{ name: string; n: number }>();
  const out: Record<string, number> = Object.fromEntries(names.map((n) => [n, 0]));
  for (const r of rows.results) out[r.name] = r.n;
  return out;
}

// Count one more against a limit. Past `cap`, the counter's CHECK fails and
// aborts the batch the bump is in, so parallel requests can't all pass a
// limit that each read as not yet reached.
function bump(env: Env, day: string, name: string, cap: number): D1PreparedStatement {
  return env.DB.prepare("INSERT INTO counters (name, day, n, cap) VALUES (?, ?, 1, ?) ON CONFLICT (name, day) DO UPDATE SET n = n + 1")
    .bind(name, day, cap);
}

// What a failed batch ran into: a counter at its cap, a unique row that a
// parallel request wrote first, or something else, which is thrown on.
function conflict(e: unknown): "limit" | "duplicate" {
  const message = e instanceof Error ? e.message : String(e);
  if (message.includes("CHECK constraint failed")) return "limit";
  if (message.includes("UNIQUE constraint failed")) return "duplicate";
  throw e;
}

// The game a verdict counts towards for brigading: its appid, or its GOG id
// when it has none.
function gameWhere(v: { steam_appid?: number; gog_id?: string }): [string, unknown] {
  return v.steam_appid !== undefined ? ["steam_appid = ?", v.steam_appid] : ["steam_appid IS NULL AND gog_id = ?", v.gog_id];
}

async function brigaded(env: Env, v: Accepted, day: string): Promise<boolean> {
  const [where, id] = gameWhere(v);
  const row = (await env.DB.prepare(
    `SELECT
       SUM(CASE WHEN created_day = ? THEN 1 ELSE 0 END) AS today,
       SUM(CASE WHEN created_day < ? THEN 1 ELSE 0 END) AS before
     FROM verdicts WHERE ${where} AND created_day >= ?`,
  )
    .bind(day, day, id, addDays(day, -L.BRIGADE_BASELINE_DAYS))
    .first()) as { today: number | null; before: number | null };
  const todayN = (row.today ?? 0) + 1;
  const before = row.before ?? 0;
  const average = before / L.BRIGADE_BASELINE_DAYS;
  return todayN > L.BRIGADE_PER_DAY || (before > 0 && todayN >= L.BRIGADE_FLOOR && todayN > L.BRIGADE_FACTOR * average);
}

// The key's earlier verdict about the same game, if any: one with the same
// appid, or, when either has none, the same GOG id. Withdrawn and removed
// verdicts are never replaced; sending again makes a new one.
async function earlier(env: Env, keyHash: string, v: Accepted): Promise<{ id: string; state: string } | null> {
  const byAppid =
    v.steam_appid !== undefined
      ? await env.DB.prepare("SELECT id, state FROM verdicts WHERE key_hash = ? AND state IN ('live', 'held') AND steam_appid = ?")
          .bind(keyHash, v.steam_appid)
          .first<{ id: string; state: string }>()
      : null;
  if (byAppid || v.gog_id === undefined) return byAppid;
  const either = v.steam_appid === undefined ? "" : "AND steam_appid IS NULL";
  return env.DB.prepare(`SELECT id, state FROM verdicts WHERE key_hash = ? AND state IN ('live', 'held') AND gog_id = ? ${either}`)
    .bind(keyHash, v.gog_id)
    .first<{ id: string; state: string }>();
}

function answers(v: Accepted): unknown[] {
  const list = (l: string[]) => (l.length > 0 ? JSON.stringify(l) : null);
  return [
    v.steam_appid ?? null,
    v.gog_id ?? null,
    v.outcome,
    v.worth,
    v.hours ?? null,
    v.clicked ?? null,
    v.sessions ?? null,
    v.limited_time ?? null,
    list(v.factors),
    v.stop_point ?? null,
    v.stop_note ?? null,
    list(v.stopped_for),
    v.text ?? null,
    v.vocab,
    v.licence,
    v.consent,
    v.client,
  ];
}

const ANSWER_COLUMNS =
  "steam_appid, gog_id, outcome, worth, hours, clicked, sessions, limited_time, factors, stop_point, stop_note, stopped_for, text, vocab, licence, consent, client_version";

// POST /v1/verdicts
export async function send(req: Request, env: Env): Promise<Response> {
  const keyHash = await authenticate(req);
  if (keyHash instanceof Response) return keyHash;
  const day = today();
  if (await blocked(env, keyHash)) return failure(403, "this key is blocked from sending verdicts");
  const parsed = await body(req);
  if (parsed instanceof Response) return parsed;
  const { verdict, problems } = readVerdict(parsed);
  if (!verdict || problems.length > 0) return refused(problems);
  const v = verdict as Accepted;
  const address = await addressName(env, req, day);
  return store(env, keyHash, v, address, day, true);
}

const ADDRESS_FULL = "too many verdicts and reports from this address today";
const NEW_FULL = `at most ${L.NEW_PER_DAY} new verdicts a day`;
const REPLACED_FULL = `at most ${L.REPLACED_PER_DAY} changed verdicts a day`;

// Write the verdict, or replace the key's earlier one about the game. When
// a parallel send about the same game wrote its verdict first, this one is
// tried again once, and replaces it.
async function store(
  env: Env,
  keyHash: string,
  v: Accepted,
  address: string,
  day: string,
  retry: boolean,
): Promise<Response> {
  const was = await earlier(env, keyHash, v);
  const newName = `key:${keyHash}:new`;
  const replacedName = `key:${keyHash}:replaced`;
  const c = await counts(env, day, [address, newName, replacedName]);
  const wait = secondsToMidnight();
  // Checked here for the answer's sake; the bumps below are what hold the
  // limits when requests race.
  if (c[address] >= L.ADDRESS_WRITES_PER_DAY) return tooMany(ADDRESS_FULL, wait);
  if (was) {
    if (c[replacedName] >= L.REPLACED_PER_DAY) return tooMany(REPLACED_FULL, wait);
  } else {
    if (c[newName] >= L.NEW_PER_DAY) return tooMany(NEW_FULL, wait);
  }

  // A held verdict stays held when it's replaced, so replacing can't be
  // used to skip the hold.
  const state = was?.state === "held" || (await brigaded(env, v, day)) ? "held" : "live";
  const id = was?.id ?? newVerdictId();
  const writes = was ? replace(env, v, state, day, id) : [insert(env, keyHash, v, state, day, id)];
  try {
    await env.DB.batch([
      env.DB.prepare("INSERT INTO keys (key_hash) VALUES (?) ON CONFLICT DO NOTHING").bind(keyHash),
      bump(env, day, address, L.ADDRESS_WRITES_PER_DAY),
      was ? bump(env, day, replacedName, L.REPLACED_PER_DAY) : bump(env, day, newName, L.NEW_PER_DAY),
      ...writes,
    ]);
  } catch (e) {
    if (conflict(e) === "duplicate") {
      // Only once: a second clash would mean something other than a race.
      if (!retry) throw e;
      return store(env, keyHash, v, address, day, false);
    }
    // Another request took the last of a limit between the check and the
    // write: say which.
    const now = await counts(env, day, [address]);
    return tooMany(now[address] >= L.ADDRESS_WRITES_PER_DAY ? ADDRESS_FULL : was ? REPLACED_FULL : NEW_FULL, wait);
  }
  return was ? json(200, { id, state, replaced: true }) : json(201, { id, state, replaced: false });
}

function insert(env: Env, keyHash: string, v: Accepted, state: string, day: string, id: string): D1PreparedStatement {
  const marks = ANSWER_COLUMNS.split(", ")
    .map(() => "?")
    .join(", ");
  return env.DB.prepare(`INSERT INTO verdicts (id, key_hash, ${ANSWER_COLUMNS}, state, created_day, updated_day) VALUES (?, ?, ${marks}, ?, ?, ?)`).bind(
    id,
    keyHash,
    ...answers(v),
    state,
    day,
    day,
  );
}

// Replacing keeps a held or hidden text as it is unless the text or the
// stopping note changed, so sending the same text again can't bring it
// back. A changed text is shown, and the reports about the old one are
// cleared.
function replace(env: Env, v: Accepted, state: string, day: string, id: string): D1PreparedStatement[] {
  const same = "text IS ? AND stop_note IS ?";
  const typed = [v.text ?? null, v.stop_note ?? null];
  const sets = ANSWER_COLUMNS.split(", ")
    .map((col) => `${col} = ?`)
    .join(", ");
  return [
    // Before the update, while the old text is still there to compare.
    env.DB.prepare(`DELETE FROM reports WHERE verdict_id = ? AND NOT EXISTS (SELECT 1 FROM verdicts WHERE id = ? AND ${same})`).bind(id, id, ...typed),
    // SQLite's SET reads the row as it was, so the CASE sees the old text.
    env.DB.prepare(`UPDATE verdicts SET ${sets}, text_state = CASE WHEN ${same} THEN text_state ELSE 'shown' END, state = ?, updated_day = ? WHERE id = ?`).bind(
      ...answers(v),
      ...typed,
      state,
      day,
      id,
    ),
  ];
}

function refused(problems: Problem[]): Response {
  const first = problems[0];
  const error = first ? `that isn't a verdict the service can take: ${first.field} ${first.why}` : "that isn't a verdict";
  return failure(400, error, { problems });
}

// DELETE /v1/verdicts/{id}
export async function withdraw(req: Request, env: Env, id: string): Promise<Response> {
  const keyHash = await authenticate(req);
  if (keyHash instanceof Response) return keyHash;
  if (!isVerdictId(id)) return failure(404, "no such verdict of this key");
  const row = await env.DB.prepare("SELECT state FROM verdicts WHERE id = ? AND key_hash = ?").bind(id, keyHash).first<{ state: string }>();
  if (!row) return failure(404, "no such verdict of this key");
  await env.DB.prepare("UPDATE verdicts SET state = 'withdrawn', updated_day = ? WHERE id = ? AND state IN ('live', 'held')").bind(today(), id).run();
  return json(204, null);
}

// DELETE /v1/verdicts
export async function withdrawAll(req: Request, env: Env): Promise<Response> {
  const keyHash = await authenticate(req);
  if (keyHash instanceof Response) return keyHash;
  const r = await env.DB.prepare("UPDATE verdicts SET state = 'withdrawn', updated_day = ? WHERE key_hash = ? AND state IN ('live', 'held')")
    .bind(today(), keyHash)
    .run();
  return json(200, { withdrawn: r.meta.changes ?? 0 });
}

interface Row {
  id: string;
  steam_appid: number | null;
  gog_id: string | null;
  outcome: string | null;
  worth: string | null;
  hours: string | null;
  clicked: string | null;
  sessions: string | null;
  limited_time: string | null;
  factors: string | null;
  stop_point: string | null;
  stop_note: string | null;
  stopped_for: string | null;
  text: string | null;
  vocab: number | null;
  licence: string | null;
  consent: number | null;
  state: string;
  created_day: string;
  updated_day: string;
}

// GET /v1/verdicts/mine: the key's verdicts with their fields, so a restored
// install can show what it sent. A withdrawn verdict is listed until the
// daily run deletes its answers.
export async function mine(req: Request, env: Env): Promise<Response> {
  const keyHash = await authenticate(req);
  if (keyHash instanceof Response) return keyHash;
  const rows = await env.DB.prepare(
    `SELECT id, ${ANSWER_COLUMNS.replace(", client_version", "")}, state, created_day, updated_day
     FROM verdicts WHERE key_hash = ? AND outcome IS NOT NULL ORDER BY created_day, id`,
  )
    .bind(keyHash)
    .all<Row>();
  const verdicts = rows.results.map((r) => {
    const out: Record<string, unknown> = { id: r.id, state: r.state, created_day: r.created_day, updated_day: r.updated_day };
    for (const [k, v] of Object.entries(r)) {
      if (k in out || v === null) continue;
      out[k] = k === "factors" || k === "stopped_for" ? JSON.parse(v as string) : v;
    }
    return out;
  });
  return json(200, { verdicts });
}

// POST /v1/reports
export async function report(req: Request, env: Env): Promise<Response> {
  const keyHash = await authenticate(req);
  if (keyHash instanceof Response) return keyHash;
  const day = today();
  if (await blocked(env, keyHash)) return failure(403, "this key is blocked from reporting");
  const parsed = await body(req);
  if (parsed instanceof Response) return parsed;
  const problems: Problem[] = [];
  if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) {
    problems.push({ field: "body", why: "isn't a report: a JSON object" });
  } else {
    const r = parsed as Record<string, unknown>;
    for (const k of Object.keys(r)) if (k !== "verdict_id" && k !== "reason") problems.push({ field: k, why: "isn't a field of a report" });
    if (typeof r.verdict_id !== "string" || !isVerdictId(r.verdict_id)) problems.push({ field: "verdict_id", why: "isn't a verdict id" });
    if (typeof r.reason !== "string" || !REASON_CODES.has(r.reason)) {
      problems.push({ field: "reason", why: `is one of ${[...REASON_CODES].join(", ")}` });
    }
  }
  if (problems.length > 0) return failure(400, "that isn't a report", { problems });
  const { verdict_id: id, reason } = parsed as { verdict_id: string; reason: string };

  const published = await env.DB.prepare("SELECT text_state FROM verdicts WHERE id = ? AND state = 'live' AND published_day IS NOT NULL")
    .bind(id)
    .first<{ text_state: string }>();
  if (!published) return failure(404, "no published verdict has that id");
  // A key's report counts once, so reporting again changes nothing.
  const already = await env.DB.prepare("SELECT 1 AS n FROM reports WHERE verdict_id = ? AND key_hash = ?").bind(id, keyHash).first();
  if (already) return json(202, {});
  const address = await addressName(env, req, day);
  const reportsName = `key:${keyHash}:reports`;
  const c = await counts(env, day, [address, reportsName]);
  const wait = secondsToMidnight();
  const reportsFull = `at most ${L.REPORTS_PER_DAY} reports a day`;
  if (c[address] >= L.ADDRESS_WRITES_PER_DAY) return tooMany(ADDRESS_FULL, wait);
  if (c[reportsName] >= L.REPORTS_PER_DAY) return tooMany(reportsFull, wait);

  try {
    await env.DB.batch([
      env.DB.prepare("INSERT INTO reports (verdict_id, key_hash, reason, day) VALUES (?, ?, ?, ?)").bind(id, keyHash, reason, day),
      bump(env, day, address, L.ADDRESS_WRITES_PER_DAY),
      bump(env, day, reportsName, L.REPORTS_PER_DAY),
      // Three keys' reports hold the text, not the answers, until a
      // moderator looks. Keys cost nothing to make, so only the reports of
      // keys with a published verdict of their own count towards that, and
      // the moderators' page lists the rest.
      env.DB.prepare(
        `UPDATE verdicts SET text_state = 'held'
         WHERE id = ? AND text_state = 'shown' AND (text IS NOT NULL OR stop_note IS NOT NULL)
           AND (SELECT COUNT(*) FROM reports r WHERE r.verdict_id = ? AND EXISTS (
                  SELECT 1 FROM verdicts mine
                  WHERE mine.key_hash = r.key_hash AND mine.state = 'live' AND mine.published_day IS NOT NULL)) >= ?`,
      ).bind(id, id, L.REPORTS_TO_HOLD),
    ]);
  } catch (e) {
    // The same report sent twice at once: the other one counted.
    if (conflict(e) === "duplicate") return json(202, {});
    const now = await counts(env, day, [address]);
    return tooMany(now[address] >= L.ADDRESS_WRITES_PER_DAY ? ADDRESS_FULL : reportsFull, wait);
  }
  return json(202, {});
}
