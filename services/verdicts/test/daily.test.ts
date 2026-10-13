// The daily run in Miniflare: what the snapshot holds and leaves out, the
// files in R2 and how they're served, and the fixture UpNext's Rust reader
// checks in `cargo test` (tests/fixtures/verdicts/service/).

import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";
import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { type Service, REPO, daysAgo, key, post, start, today, verdict } from "./harness";

const FIXTURES = join(REPO, "tests/fixtures/verdicts");
const SERVICE_FIXTURE = join(FIXTURES, "service");
const DATE = "2026-10-09";

let s: Service;

beforeAll(async () => {
  s = await start();
});
afterAll(async () => {
  await s.dispose();
});
beforeEach(async () => {
  await s.reset();
});

interface Seed {
  id: string;
  key?: string;
  steam_appid?: number;
  gog_id?: string;
  outcome?: string;
  worth?: string;
  hours?: string;
  clicked?: string;
  sessions?: string;
  limited_time?: string;
  factors?: string[];
  stop_point?: string;
  stop_note?: string;
  stopped_for?: string[];
  text?: string;
  text_state?: string;
  state?: string;
  vocab?: number;
  day?: string;
}

async function seed(rows: Seed[]) {
  // Each verdict its own key unless one is named, as one key has one
  // verdict a game.
  const keyOf = (r: Seed) => r.key ?? `k:${r.id}`;
  const keys = new Set(rows.map(keyOf));
  await s.db.batch([
    ...[...keys].map((k) => s.db.prepare("INSERT INTO keys (key_hash) VALUES (?) ON CONFLICT DO NOTHING").bind(k)),
    ...rows.map((r) =>
      s.db
        .prepare(
          `INSERT INTO verdicts (id, key_hash, steam_appid, gog_id, outcome, worth, hours, clicked, sessions, limited_time, factors,
             stop_point, stop_note, stopped_for, text, text_state, state, vocab, licence, consent, client_version, created_day, updated_day)
           VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'CC0-1.0', 1, '0.1.0', ?, ?)`,
        )
        .bind(
          r.id,
          keyOf(r),
          r.steam_appid ?? null,
          r.gog_id ?? null,
          r.outcome ?? "finished",
          r.worth ?? "yes",
          r.hours ?? null,
          r.clicked ?? null,
          r.sessions ?? null,
          r.limited_time ?? null,
          r.factors?.length ? JSON.stringify(r.factors) : null,
          r.stop_point ?? null,
          r.stop_note ?? null,
          r.stopped_for?.length ? JSON.stringify(r.stopped_for) : null,
          r.text ?? null,
          r.text_state ?? "shown",
          r.state ?? "live",
          r.vocab ?? 1,
          r.day ?? "2026-09-15",
          r.day ?? "2026-09-15",
        ),
    ),
  ]);
}

async function bytes(name: string): Promise<Uint8Array<ArrayBuffer>> {
  const o = await s.bucket.get(name);
  if (!o) throw new Error(`${name} isn't in R2`);
  return new Uint8Array(await o.arrayBuffer());
}

async function lines(name: string): Promise<Record<string, unknown>[]> {
  return gunzipSync(await bytes(name))
    .toString("utf8")
    .split("\n")
    .filter((l) => l !== "")
    .map((l) => JSON.parse(l) as Record<string, unknown>);
}

const id = (n: number) => `v${String(n).padStart(25, "a")}`;

// The fixture's verdicts as rows: the Rust fixture's full file, each sent by
// a key of its own on the first of its month.
function fixtureRows(): Seed[] {
  return gunzipSync(readFileSync(join(FIXTURES, `verdicts-all-${DATE}.jsonl.gz`)))
    .toString("utf8")
    .split("\n")
    .slice(1)
    .filter((l) => l !== "")
    .map((l, i) => {
      const { month, ...v } = JSON.parse(l) as Seed & { month: string };
      return { ...v, key: `key${i}`, day: `${month}-01` };
    });
}

describe("the snapshot", () => {
  it("is the fixture's snapshot for the fixture's verdicts, and the Rust reader's fixture is current", async () => {
    await seed(fixtureRows());
    await s.scheduled(DATE);
    const manifest = JSON.parse(new TextDecoder().decode(await bytes("verdicts.json"))) as {
      summary: { file: string; bytes: number; sha256: string };
      all: { file: string; bytes: number; sha256: string };
      games: number;
      verdicts: number;
    };
    expect(manifest).toMatchObject({ upnext_verdicts: 1, date: DATE, games: 4, verdicts: 37, licence: "CC0-1.0" });
    const summary = await bytes(manifest.summary.file);
    const all = await bytes(manifest.all.file);
    expect(manifest.summary.bytes).toBe(summary.length);
    expect(manifest.all.bytes).toBe(all.length);
    const hash = async (b: Uint8Array<ArrayBuffer>) => [...new Uint8Array(await crypto.subtle.digest("SHA-256", b))].map((x) => x.toString(16).padStart(2, "0")).join("");
    expect(manifest.summary.sha256).toBe(await hash(summary));
    expect(manifest.all.sha256).toBe(await hash(all));

    // The summary is UpNext's own, byte for byte once unpacked.
    const rustSummary = gunzipSync(readFileSync(join(FIXTURES, `verdicts-summary-${DATE}.jsonl.gz`))).toString("utf8");
    expect(gunzipSync(summary).toString("utf8")).toBe(rustSummary);

    const files: Record<string, Uint8Array<ArrayBuffer>> = {
      "verdicts.json": await bytes("verdicts.json"),
      [manifest.summary.file]: summary,
      [manifest.all.file]: all,
    };
    if (process.env.UPNEXT_UPDATE_VERDICTS_SERVICE === "1") {
      mkdirSync(SERVICE_FIXTURE, { recursive: true });
      for (const [name, data] of Object.entries(files)) writeFileSync(join(SERVICE_FIXTURE, name), data);
    }
    // gzip's bytes may differ between runtimes, so the committed files are
    // compared unpacked; `cargo test` reads them with UpNext's reader.
    for (const name of [manifest.summary.file, manifest.all.file]) {
      expect(gunzipSync(readFileSync(join(SERVICE_FIXTURE, name))).toString("utf8"), `${name} is current; rerun with UPNEXT_UPDATE_VERDICTS_SERVICE=1`).toBe(
        gunzipSync(files[name]).toString("utf8"),
      );
    }
  });

  it("leaves out withdrawn, removed, held and blocked verdicts, held and hidden texts, and games under three", async () => {
    await seed([
      { id: id(1), steam_appid: 1, text: "Shown text.", stop_point: "at_credits", stop_note: "after chapter 3" },
      { id: id(2), steam_appid: 1, text: "Held text.", text_state: "held" },
      { id: id(3), steam_appid: 1, text: "Hidden text.", text_state: "hidden" },
      { id: id(4), steam_appid: 1, state: "withdrawn", text: "Withdrawn." },
      { id: id(5), steam_appid: 1, state: "removed" },
      { id: id(6), steam_appid: 1, state: "held", day: daysAgo(1) },
      { id: id(7), steam_appid: 1, key: "blocked" },
      { id: id(8), steam_appid: 2 },
      { id: id(9), steam_appid: 2 },
    ]);
    await s.db.prepare("UPDATE keys SET blocked = 1 WHERE key_hash = 'blocked'").run();
    await s.scheduled(today());
    const all = await lines(`verdicts-all-${today()}.jsonl.gz`);
    expect(all.slice(1).map((v) => v.id)).toEqual([id(1), id(2), id(3), id(8), id(9)]);
    const texts = all.slice(1).map((v) => [v.text ?? null, v.stop_note ?? null]);
    expect(texts).toEqual([
      ["Shown text.", "after chapter 3"],
      [null, null],
      [null, null],
      [null, null],
      [null, null],
    ]);
    const summary = await lines(`verdicts-summary-${today()}.jsonl.gz`);
    expect(summary).toHaveLength(2);
    expect(summary[1]).toMatchObject({ steam_appid: 1, n: 3 });
    expect((summary[1].texts as unknown[]).length).toBe(1);

    // Nothing published names a key or a day.
    const raw = gunzipSync(await bytes(`verdicts-all-${today()}.jsonl.gz`)).toString("utf8");
    expect(raw).not.toContain("key_hash");
    expect(raw).not.toContain("blocked");
    expect(raw).not.toContain("client");
    expect(raw.split("\n").slice(1).join("\n")).not.toMatch(/\d{4}-\d{2}-\d{2}/);
    for (const v of all.slice(1)) expect(v.month).toMatch(/^\d{4}-\d{2}$/);

    // The withdrawn and removed verdicts' answers and text are deleted;
    // their id, game, key hash and days stay.
    const gone = await s.db.prepare("SELECT * FROM verdicts WHERE state IN ('withdrawn', 'removed') ORDER BY id").all<Record<string, unknown>>();
    for (const r of gone.results) {
      expect(r).toMatchObject({ steam_appid: 1, outcome: null, worth: null, text: null, licence: null, client_version: null });
      expect(r.key_hash).toBe(`k:${r.id}`);
    }
  });

  it("leaves out a text that fails today's checks, keeping its answers", async () => {
    // Sent before a check got stricter: a zero-width space hid the link.
    await seed([
      { id: id(1), steam_appid: 4, text: "my site is example\u200b.com", stop_point: "anytime", stop_note: "fine" },
      { id: id(2), steam_appid: 4, text: "Fine in short sessions." },
      { id: id(3), steam_appid: 4, stop_point: "at_credits", stop_note: "after\u202e chapter 3" },
    ]);
    await s.scheduled(today());
    const all = await lines(`verdicts-all-${today()}.jsonl.gz`);
    expect(all.slice(1).map((v) => [v.id, v.text ?? null, v.stop_note ?? null])).toEqual([
      [id(1), null, "fine"],
      [id(2), "Fine in short sessions.", null],
      [id(3), null, null],
    ]);
    const summary = await lines(`verdicts-summary-${today()}.jsonl.gz`);
    expect(summary[1]).toMatchObject({ steam_appid: 4, n: 3 });
  });

  it("releases held verdicts after seven days unless they were reported", async () => {
    await seed([
      { id: id(1), steam_appid: 3, state: "held", day: daysAgo(8) },
      { id: id(2), steam_appid: 3, state: "held", day: daysAgo(8) },
      { id: id(3), steam_appid: 3, state: "held", day: daysAgo(2) },
    ]);
    await s.db.prepare("INSERT INTO reports (verdict_id, key_hash, reason, day) VALUES (?, 'r', 'spam', ?)").bind(id(2), daysAgo(3)).run();
    await s.scheduled(today());
    const states = await s.db.prepare("SELECT id, state FROM verdicts ORDER BY id").all();
    expect(states.results.map((r) => r.state)).toEqual(["live", "held", "held"]);
  });

  it("drops earlier days' counters and salts, and dated files older than six days", async () => {
    await s.fetch("/v1/verdicts", post(key(1), verdict()));
    await s.db.batch([
      s.db.prepare("INSERT INTO counters (name, day, n, cap) VALUES ('old', ?, 5, 60)").bind(daysAgo(1)),
      s.db.prepare("INSERT INTO salts (day, salt) VALUES (?, 'x')").bind(daysAgo(1)),
    ]);
    for (let d = 0; d < 8; d++) {
      await s.bucket.put(`verdicts-summary-${daysAgo(d + 1)}.jsonl.gz`, "x");
      await s.bucket.put(`verdicts-all-${daysAgo(d + 1)}.jsonl.gz`, "x");
    }
    await s.scheduled(today());
    expect((await s.db.prepare("SELECT day FROM counters").all()).results.every((r) => r.day === today())).toBe(true);
    expect((await s.db.prepare("SELECT day FROM salts").all()).results.map((r) => r.day)).toEqual([today()]);
    const kept = (await s.bucket.list()).objects.map((o) => o.key).filter((k) => k.startsWith("verdicts-summary-")).sort();
    expect(kept).toEqual([0, 1, 2, 3, 4, 5].map((d) => `verdicts-summary-${daysAgo(d)}.jsonl.gz`).reverse());
  });

  it("makes a verdict reportable once it's published", async () => {
    const sent = (await (await s.fetch("/v1/verdicts", post(key(1), verdict({ text: "A text." })))).json()) as { id: string };
    const report = () => s.fetch("/v1/reports", post(key(2), { verdict_id: sent.id, reason: "spam" }));
    expect((await report()).status).toBe(404);
    await s.scheduled(today());
    expect((await report()).status).toBe(202);
  });
});

describe("serving the snapshot", () => {
  it("serves the manifest with an ETag and the dated files cached for at most a day", async () => {
    await seed([{ id: id(1), steam_appid: 1 }]);
    await s.scheduled(today());
    const m = await s.fetch("/verdicts.json");
    expect(m.status).toBe(200);
    expect(m.headers.get("content-type")).toBe("application/json");
    expect(m.headers.get("cache-control")).toBe("public, max-age=300");
    const etag = m.headers.get("etag") as string;
    expect(etag).toBeTruthy();
    const body = (await m.json()) as { summary: { file: string; bytes: number } };
    const again = await s.fetch("/verdicts.json", { headers: { "if-none-match": etag } });
    expect(again.status).toBe(304);

    const f = await s.fetch(`/${body.summary.file}`);
    expect(f.status).toBe(200);
    expect(f.headers.get("content-type")).toBe("application/gzip");
    expect(f.headers.get("cache-control")).toBe("public, max-age=86400");
    expect(f.headers.get("content-encoding")).toBeNull();
    expect((await f.arrayBuffer()).byteLength).toBe(body.summary.bytes);
    expect((await s.fetch("/verdicts-summary-1999-01-01.jsonl.gz")).status).toBe(404);
    expect((await s.fetch("/secret.txt")).status).toBe(404);
  });
});
