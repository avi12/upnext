// The moderators' pages (docs/dev/VERDICTS-SERVICE.md, "Moderating"), behind Cloudflare Access:
// reported texts, held games, the newest texts and a search by verdict id,
// with the actions hide or show a text, dismiss its reports, remove a
// verdict, release held verdicts, and block or unblock the key that wrote
// one. Every action is
// logged with the moderator and the day. Key hashes never appear on a page;
// a key is acted on through one of its verdicts.

import { moderator } from "./access";
import type { Env } from "./env";
import { page, escape as e } from "./html";
import { addDays, isVerdictId, today } from "./util";

interface Listed {
  id: string;
  steam_appid: number | null;
  gog_id: string | null;
  outcome: string | null;
  worth: string | null;
  text: string | null;
  stop_note: string | null;
  text_state: string;
  state: string;
  created_day: string;
  updated_day: string;
  blocked: number;
  reports: string | null;
}

const LISTED = `SELECT v.id, v.steam_appid, v.gog_id, v.outcome, v.worth, v.text, v.stop_note, v.text_state, v.state,
    v.created_day, v.updated_day, k.blocked,
    (SELECT group_concat(reason, ', ') FROM reports r WHERE r.verdict_id = v.id) AS reports
  FROM verdicts v JOIN keys k ON k.key_hash = v.key_hash`;

const ACTIONS: Record<string, string> = {
  hide_text: "Hide the text",
  show_text: "Show the text",
  dismiss_reports: "Dismiss the reports",
  remove: "Remove the verdict",
  release: "Release",
  release_game: "Release the game's held verdicts",
  block: "Block its key",
  unblock: "Unblock its key",
};

function game(v: Listed): string {
  const parts = [];
  if (v.steam_appid !== null) parts.push(`Steam ${v.steam_appid}`);
  if (v.gog_id !== null) parts.push(`GOG ${e(v.gog_id)}`);
  return parts.join(", ");
}

function actionsFor(v: Listed): string[] {
  const out: string[] = [];
  if (v.text !== null || v.stop_note !== null) out.push(v.text_state === "hidden" ? "show_text" : "hide_text");
  if (v.text_state === "held") out.push("show_text");
  if (v.text_state === "shown" && v.reports !== null) out.push("dismiss_reports");
  if (v.state === "held") out.push("release", "release_game");
  if (v.state !== "removed" && v.state !== "withdrawn") out.push("remove");
  out.push(v.blocked ? "unblock" : "block");
  return [...new Set(out)];
}

function table(rows: Listed[], empty: string): string {
  if (rows.length === 0) return `<p class="none">${e(empty)}</p>`;
  const body = rows
    .map((v) => {
      const buttons = actionsFor(v)
        .map((a) => `<button name="action" value="${a}">${e(ACTIONS[a])}</button>`)
        .join(" ");
      return `<tr>
  <td><a href="/admin?id=${v.id}"><code>${v.id}</code></a><br>${game(v)}</td>
  <td>${e(v.outcome ?? "")}<br>${e(v.worth ?? "")}</td>
  <td>${v.text !== null ? `<p>${e(v.text)}</p>` : ""}${v.stop_note !== null ? `<p><i>Stop:</i> ${e(v.stop_note)}</p>` : ""}</td>
  <td>${e(v.state)}${v.text_state !== "shown" ? `, text ${e(v.text_state)}` : ""}${v.blocked ? ", key blocked" : ""}<br>${e(v.updated_day)}${v.reports ? `<br>Reports: ${e(v.reports)}` : ""}</td>
  <td><form method="post" action="/admin/act"><input type="hidden" name="id" value="${v.id}"><input name="note" placeholder="Note (optional)" maxlength="200"> ${buttons}</form></td>
</tr>`;
    })
    .join("\n");
  return `<table><thead><tr><th>Verdict</th><th>Answers</th><th>Text</th><th>State</th><th>Act</th></tr></thead><tbody>${body}</tbody></table>`;
}

async function list(env: Env, where: string, ...binds: unknown[]): Promise<Listed[]> {
  return (
    await env.DB.prepare(`${LISTED} WHERE ${where} ORDER BY v.updated_day DESC, v.id LIMIT 100`)
      .bind(...binds)
      .all<Listed>()
  ).results;
}

async function home(req: Request, env: Env, who: string): Promise<Response> {
  const url = new URL(req.url);
  const id = url.searchParams.get("id") ?? "";
  const day = today();
  const sections: string[] = [];
  if (id) {
    const found = isVerdictId(id) ? await list(env, "v.id = ?", id) : [];
    sections.push(`<h2>Verdict ${e(id)}</h2>${table(found, "No verdict has that id.")}`);
  }
  // Held texts, and shown ones with reports too few to hold them yet, or
  // from keys whose reports don't count towards holding (`api.ts`, `report`).
  const reported = "v.state IN ('live', 'held') AND (v.text_state = 'held' OR (v.text_state = 'shown' AND EXISTS (SELECT 1 FROM reports r WHERE r.verdict_id = v.id)))";
  sections.push(`<h2>Reported texts</h2>${table(await list(env, reported), "No text is waiting.")}`);
  sections.push(`<h2>Held verdicts</h2>${table(await list(env, "v.state = 'held'"), "No game is held.")}`);
  sections.push(
    `<h2>New texts since yesterday</h2>${table(
      await list(env, "v.updated_day >= ? AND v.state = 'live' AND v.text_state = 'shown' AND (v.text IS NOT NULL OR v.stop_note IS NOT NULL)", addDays(day, -1)),
      "No new texts.",
    )}`,
  );
  const searchForm = `<form method="get" action="/admin"><input name="id" placeholder="Verdict id" value="${e(id)}" size="30"> <button>Find</button></form>`;
  return page(
    "Backlog verdicts: moderation",
    `<p>Signed in as ${e(who)}. <a href="/admin/log">Moderation log</a></p>${searchForm}${sections.join("\n")}`,
    { admin: true },
  );
}

async function log(env: Env): Promise<Response> {
  const rows = (
    await env.DB.prepare("SELECT at, moderator, action, verdict_id, note FROM moderation_log ORDER BY rowid DESC LIMIT 200").all<{
      at: string;
      moderator: string;
      action: string;
      verdict_id: string | null;
      note: string | null;
    }>()
  ).results;
  const body = rows
    .map((r) => `<tr><td>${e(r.at)}</td><td>${e(r.moderator)}</td><td>${e(r.action)}</td><td><code>${e(r.verdict_id ?? "")}</code></td><td>${e(r.note ?? "")}</td></tr>`)
    .join("\n");
  return page(
    "Backlog verdicts: moderation log",
    `<p><a href="/admin">Back</a></p><table><thead><tr><th>Day</th><th>Moderator</th><th>Action</th><th>Verdict</th><th>Note</th></tr></thead><tbody>${body}</tbody></table>`,
    { admin: true },
  );
}

async function act(req: Request, env: Env, who: string): Promise<Response> {
  // Access signs requests in with a cookie, so a form posted from another
  // site would carry it; only the admin pages' own forms are taken.
  const origin = req.headers.get("origin");
  if (origin !== new URL(req.url).origin) return new Response("Refused: the form didn't come from these pages.", { status: 403 });
  const form = await req.formData();
  const action = String(form.get("action") ?? "");
  const id = String(form.get("id") ?? "");
  const note = String(form.get("note") ?? "").slice(0, 200) || null;
  if (!(action in ACTIONS) || !isVerdictId(id)) return new Response("Refused: no such action or verdict.", { status: 400 });
  const v = await env.DB.prepare("SELECT key_hash, steam_appid, gog_id FROM verdicts WHERE id = ?")
    .bind(id)
    .first<{ key_hash: string; steam_appid: number | null; gog_id: string | null }>();
  if (!v) return new Response("Refused: no verdict has that id.", { status: 404 });
  const db = env.DB;
  const steps: Record<string, D1PreparedStatement[]> = {
    hide_text: [db.prepare("UPDATE verdicts SET text_state = 'hidden' WHERE id = ?").bind(id)],
    show_text: [db.prepare("UPDATE verdicts SET text_state = 'shown' WHERE id = ?").bind(id), db.prepare("DELETE FROM reports WHERE verdict_id = ?").bind(id)],
    dismiss_reports: [db.prepare("DELETE FROM reports WHERE verdict_id = ?").bind(id)],
    remove: [db.prepare("UPDATE verdicts SET state = 'removed' WHERE id = ? AND state IN ('live', 'held')").bind(id)],
    release: [db.prepare("UPDATE verdicts SET state = 'live' WHERE id = ? AND state = 'held'").bind(id)],
    release_game: [
      v.steam_appid !== null
        ? db.prepare("UPDATE verdicts SET state = 'live' WHERE state = 'held' AND steam_appid = ?").bind(v.steam_appid)
        : db.prepare("UPDATE verdicts SET state = 'live' WHERE state = 'held' AND steam_appid IS NULL AND gog_id = ?").bind(v.gog_id),
    ],
    block: [db.prepare("UPDATE keys SET blocked = 1 WHERE key_hash = ?").bind(v.key_hash)],
    unblock: [db.prepare("UPDATE keys SET blocked = 0 WHERE key_hash = ?").bind(v.key_hash)],
  };
  const keyAction = action === "block" || action === "unblock";
  await db.batch([
    ...steps[action],
    db
      .prepare("INSERT INTO moderation_log (at, moderator, action, verdict_id, key_hash, note) VALUES (?, ?, ?, ?, ?, ?)")
      .bind(today(), who, action, id, keyAction ? v.key_hash : null, note),
  ]);
  return new Response(null, { status: 303, headers: { location: `/admin?id=${id}` } });
}

export async function admin(req: Request, env: Env, path: string): Promise<Response> {
  const who = await moderator(req, env);
  // A 404 says nothing about whether there are admin pages here; the
  // reason goes to the Worker's log for the operator.
  if ("refused" in who) {
    console.log(`admin refused: ${who.refused}`);
    return new Response("Not found", { status: 404 });
  }
  if (path === "/admin" && req.method === "GET") return home(req, env, who.email);
  if (path === "/admin/log" && req.method === "GET") return log(env);
  if (path === "/admin/act" && req.method === "POST") return act(req, env, who.email);
  return new Response("Not found", { status: 404 });
}
