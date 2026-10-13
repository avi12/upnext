// UpNext's backlog verdicts service: a Cloudflare Worker
// with a D1 database for verdicts and an R2 bucket for the daily snapshot.
// `docs/VERDICTS.md` is the contract it follows; `README.md` beside this
// folder says how to run, test and deploy it.

import { admin } from "./admin";
import * as api from "./api";
import { daily } from "./daily";
import type { Env } from "./env";
import { terms } from "./terms";
import { dayOf } from "./util";

const SNAPSHOT_FILE = /^\/(verdicts\.json|verdicts-(summary|all)-\d{4}-\d{2}-\d{2}\.jsonl\.gz)$/;

// The snapshot's files, served from R2 under the same address as the
// endpoint, so UpNext's one `verdicts_url` setting covers both. An
// unchanged manifest costs one small request with `If-None-Match`.
async function file(req: Request, env: Env, name: string): Promise<Response> {
  const obj = await env.SNAPSHOTS.get(name, { onlyIf: req.headers });
  if (!obj) return new Response("Not found", { status: 404 });
  const headers = new Headers();
  obj.writeHttpMetadata(headers);
  headers.set("etag", obj.httpEtag);
  // The data is public, so pages and tools in a browser may read it too.
  headers.set("access-control-allow-origin", "*");
  if (!("body" in obj)) return new Response(null, { status: 304, headers });
  return new Response(req.method === "HEAD" ? null : obj.body, { headers });
}

const INDEX = `UpNext backlog verdicts.
Players' answers about how games fit a backlog, published daily under CC0 1.0.
Snapshot: /verdicts.json. Terms and privacy: /terms.
`;

async function route(req: Request, env: Env): Promise<Response> {
  const url = new URL(req.url);
  const path = url.pathname;
  const m = req.method;
  if (path === "/v1/verdicts" && m === "POST") return api.send(req, env);
  if (path === "/v1/verdicts" && m === "DELETE") return api.withdrawAll(req, env);
  if (path === "/v1/verdicts/mine" && m === "GET") return api.mine(req, env);
  const one = /^\/v1\/verdicts\/([^/]+)$/.exec(path);
  if (one && m === "DELETE") return api.withdraw(req, env, one[1]);
  if (path === "/v1/reports" && m === "POST") return api.report(req, env);
  if (path === "/admin" || path.startsWith("/admin/")) return admin(req, env, path);
  if (m === "GET" || m === "HEAD") {
    if (path === "/terms") return terms(env);
    const f = SNAPSHOT_FILE.exec(path);
    if (f) return file(req, env, f[1]);
    if (path === "/") return new Response(INDEX, { headers: { "content-type": "text/plain; charset=utf-8" } });
  }
  if (path.startsWith("/v1/")) return api.failure(path.startsWith("/v1/verdicts") || path === "/v1/reports" ? 405 : 404, "no such endpoint");
  return new Response("Not found", { status: 404 });
}

export default {
  async fetch(req: Request, env: Env): Promise<Response> {
    try {
      return await route(req, env);
    } catch (err) {
      console.error(err);
      return api.failure(500, "the service hit an error; try again later");
    }
  },

  async scheduled(controller: ScheduledController, env: Env): Promise<void> {
    const result = await daily(env, dayOf(controller.scheduledTime));
    console.log(JSON.stringify({ daily: result }));
  },
} satisfies ExportedHandler<Env>;
