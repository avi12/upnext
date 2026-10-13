// Runs the Worker in Miniflare, Cloudflare's local runtime (workerd), with an
// in-memory D1 database and R2 bucket and the migrations applied. Nothing
// here touches the network or a Cloudflare account.

import { build } from "esbuild";
import { Miniflare, convertV4MiniflareOptions } from "miniflare";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..");
export const REPO = join(ROOT, "..", "..");

let bundled: string | undefined;

// The Worker bundled the way wrangler bundles it for workerd.
async function script(): Promise<string> {
  if (bundled) return bundled;
  const out = await build({
    entryPoints: [join(ROOT, "src/index.ts")],
    bundle: true,
    format: "esm",
    platform: "neutral",
    conditions: ["workerd", "worker", "browser"],
    mainFields: ["module", "main"],
    target: "es2023",
    write: false,
    logLevel: "silent",
  });
  bundled = out.outputFiles[0].text;
  return bundled;
}

function compatibilityDate(): string {
  const m = /^compatibility_date = "([^"]+)"/m.exec(readFileSync(join(ROOT, "wrangler.toml"), "utf8"));
  if (!m) throw new Error("wrangler.toml has no compatibility_date");
  return m[1];
}

// The migrations' statements, in order, without their comments.
export function migrations(): string[] {
  const dir = join(ROOT, "migrations");
  return readdirSync(dir)
    .filter((f) => f.endsWith(".sql"))
    .sort()
    .flatMap((f) =>
      readFileSync(join(dir, f), "utf8")
        .split("\n")
        .filter((l) => !l.trim().startsWith("--"))
        .join("\n")
        .split(";")
        .map((s) => s.trim())
        .filter((s) => s !== ""),
    );
}

// Miniflare's own types for the bindings, so the tests need no Workers globals.
export type D1Database = Awaited<ReturnType<Miniflare["getD1Database"]>>;
export type R2Bucket = Awaited<ReturnType<Miniflare["getR2Bucket"]>>;

export const TABLES = ["verdicts", "keys", "reports", "moderation_log", "counters", "salts"];

export interface Service {
  mf: Miniflare;
  db: D1Database;
  bucket: R2Bucket;
  // Every address the Worker tried to fetch. None is reached: the tests
  // never use the network.
  outbound: string[];
  fetch(path: string, init?: RequestInit): Promise<Response>;
  scheduled(day: string): Promise<void>;
  reset(): Promise<void>;
  dispose(): Promise<void>;
}

export async function start(bindings: Record<string, string> = {}): Promise<Service> {
  const outbound: string[] = [];
  const mf = new Miniflare(
    convertV4MiniflareOptions({
      modules: true,
      script: await script(),
      compatibilityDate: compatibilityDate(),
      d1Databases: ["DB"],
      r2Buckets: ["SNAPSHOTS"],
      bindings,
      outboundService: (req: Request) => {
        outbound.push(req.url);
        return new Response("the tests don't use the network", { status: 503 });
      },
    }),
  );
  const db = await mf.getD1Database("DB");
  const bucket = await mf.getR2Bucket("SNAPSHOTS");
  await db.batch(migrations().map((s) => db.prepare(s)));
  return {
    mf,
    db,
    bucket,
    outbound,
    fetch: (path, init) => mf.dispatchFetch(`https://verdicts.test${path}`, init as never) as unknown as Promise<Response>,
    async scheduled(day) {
      const worker = await mf.getWorker();
      const r = await worker.scheduled({ cron: "17 3 * * *", scheduledTime: new Date(`${day}T03:17:00Z`) });
      if (r.outcome !== "ok") throw new Error(`the daily run failed: ${r.outcome}`);
    },
    async reset() {
      await db.batch(TABLES.map((t) => db.prepare(`DELETE FROM ${t}`)));
      const listed = await bucket.list();
      if (listed.objects.length > 0) await bucket.delete(listed.objects.map((o) => o.key));
    },
    dispose: () => mf.dispose(),
  };
}

// A player's key: the prefix and 64 hex characters.
export function key(n: number): string {
  return `upnext_v_${n.toString(16).padStart(64, "0")}`;
}

export function today(): string {
  return new Date().toISOString().slice(0, 10);
}

export function daysAgo(n: number): string {
  return new Date(Date.now() - n * 86_400_000).toISOString().slice(0, 10);
}

// A verdict UpNext would send, with whatever the test changes.
export function verdict(over: Record<string, unknown> = {}): Record<string, unknown> {
  return { steam_appid: 620, outcome: "finished", worth: "yes", hours: "10_20", vocab: 1, licence: "CC0-1.0", consent: 1, client: "0.1.0", ...over };
}

export function post(k: string | undefined, body: unknown, headers: Record<string, string> = {}): RequestInit {
  return {
    method: "POST",
    headers: { "content-type": "application/json", ...(k ? { authorization: `Bearer ${k}` } : {}), ...headers },
    body: typeof body === "string" ? body : JSON.stringify(body),
  };
}

export function del(k: string): RequestInit {
  return { method: "DELETE", headers: { authorization: `Bearer ${k}` } };
}

export function get(k: string): RequestInit {
  return { headers: { authorization: `Bearer ${k}` } };
}

export async function sha256Hex(s: string): Promise<string> {
  const d = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(s));
  return [...new Uint8Array(d)].map((b) => b.toString(16).padStart(2, "0")).join("");
}
