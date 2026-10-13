// The endpoint in Miniflare: sending, replacing, withdrawing, listing,
// reporting, the limits, holding a brigaded game and blocked keys.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { clientNetwork } from "../src/util";
import { type Service, daysAgo, del, get, key, post, sha256Hex, start, today, verdict } from "./harness";

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

async function send(k: string, over: Record<string, unknown> = {}, headers: Record<string, string> = {}) {
  const r = await s.fetch("/v1/verdicts", post(k, verdict(over), headers));
  return { status: r.status, body: (await r.json()) as Record<string, unknown>, headers: r.headers };
}

// Mark verdicts as in a published snapshot, as the daily run does.
async function publish() {
  await s.db.prepare("UPDATE verdicts SET published_day = ? WHERE state = 'live'").bind(today()).run();
}

describe("sending", () => {
  it("takes a verdict, keeps only the key's hash and replaces it on a second send", async () => {
    const k = key(1);
    const first = await send(k, { text: "Fine in short sessions." });
    expect(first.status).toBe(201);
    expect(first.body).toMatchObject({ state: "live", replaced: false });
    expect(first.body.id).toMatch(/^[a-z2-7]{26}$/);
    const row = await s.db.prepare("SELECT * FROM verdicts").first<Record<string, unknown>>();
    expect(row).toMatchObject({ key_hash: await sha256Hex(k), steam_appid: 620, outcome: "finished", text: "Fine in short sessions.", client_version: "0.1.0" });
    expect(JSON.stringify(await s.db.prepare("SELECT * FROM keys").all())).not.toContain(k);

    const second = await send(k, { worth: "for_some" });
    expect(second.status).toBe(200);
    expect(second.body).toEqual({ id: first.body.id, state: "live", replaced: true });
    const rows = await s.db.prepare("SELECT worth, text FROM verdicts").all();
    expect(rows.results).toEqual([{ worth: "for_some", text: null }]);
  });

  it("keeps a hidden or held text hidden when it's sent again, and shows a changed one", async () => {
    const k = key(9);
    const v = await send(k, { text: "Fine in short sessions." });
    const id = v.body.id as string;
    for (const state of ["hidden", "held"]) {
      await s.db.batch([
        s.db.prepare("UPDATE verdicts SET text_state = ?").bind(state),
        s.db.prepare("INSERT INTO reports (verdict_id, key_hash, reason, day) VALUES (?, 'r', 'spam', ?) ON CONFLICT DO NOTHING").bind(id, today()),
      ]);
      expect((await send(k, { text: "Fine in short sessions.", worth: "no" })).status).toBe(200);
      expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe(state);
      expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(1);
    }
    expect((await send(k, { text: "Fine in short sessions, mostly." })).status).toBe(200);
    expect(await s.db.prepare("SELECT text, text_state FROM verdicts").first()).toEqual({ text: "Fine in short sessions, mostly.", text_state: "shown" });
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(0);
  });

  it("replaces by GOG id when either verdict has no appid", async () => {
    const k = key(2);
    const first = await send(k, { steam_appid: undefined, gog_id: "1207664663" });
    const second = await send(k, { steam_appid: 292030, gog_id: "1207664663" });
    expect(second.body).toMatchObject({ id: first.body.id, replaced: true });
    const other = await send(k, { steam_appid: 400, gog_id: "1207664663" });
    expect(other.body.replaced).toBe(false);
  });

  it("refuses what isn't a key, JSON, small or a verdict", async () => {
    expect((await s.fetch("/v1/verdicts", post(undefined, verdict()))).status).toBe(401);
    expect((await s.fetch("/v1/verdicts", post("upnext_v_short", verdict()))).status).toBe(401);
    expect((await s.fetch("/v1/verdicts", post(key(3), verdict(), { "content-type": "text/plain" }))).status).toBe(415);
    expect((await s.fetch("/v1/verdicts", post(key(3), "{not json"))).status).toBe(400);
    const big = await s.fetch("/v1/verdicts", post(key(3), verdict({ text: "a".repeat(9000) })));
    expect(big.status).toBe(413);

    const r = await s.fetch("/v1/verdicts", post(key(3), verdict({ steam_id: "76561197960287930", licence: "CC-BY-4.0" })));
    expect(r.status).toBe(400);
    const b = (await r.json()) as { error: string; problems: unknown[] };
    expect(b.problems).toEqual([{ field: "steam_id", why: "isn't a field of a verdict" }]);

    const t = await s.fetch("/v1/verdicts", post(key(3), verdict({ text: "add me at steamcommunity.com/id/someone", consent: 0 })));
    expect(((await t.json()) as { problems: unknown[] }).problems).toEqual([
      { field: "text", why: "has a Steam profile or link; take it out to send" },
      { field: "consent", why: "consent version 0 isn't accepted (1 to 1)" },
    ]);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM verdicts").first("n")).toBe(0);
  });

  it("stops reading a body sent without a length once it's past the limit", async () => {
    // A chunked body of 4 MB, made as it's read. The service stops reading
    // past the limit and answers, which cuts the upload off, long before the
    // rest has been sent. Sent over HTTP to workerd, since a streamed body
    // doesn't pass through dispatchFetch.
    let sent = 0;
    const total = 4 * 1024 * 1024;
    const chunked = new ReadableStream<Uint8Array>({
      pull(controller) {
        if (sent >= total) return controller.close();
        sent += 1024;
        controller.enqueue(new Uint8Array(1024).fill(0x20));
      },
    });
    let answer: number | "cut off";
    try {
      const r = await fetch(new URL("/v1/verdicts", await s.mf.ready), {
        method: "POST",
        headers: { "content-type": "application/json", authorization: `Bearer ${key(3)}` },
        body: chunked,
        duplex: "half",
      } as RequestInit);
      answer = r.status;
      await r.body?.cancel();
    } catch {
      answer = "cut off";
    }
    expect([413, "cut off"]).toContain(answer);
    expect(sent).toBeLessThan(total / 4);
  });

  it("refuses unknown paths and methods with JSON errors", async () => {
    expect((await s.fetch("/v1/verdicts", { method: "PUT" })).status).toBe(405);
    expect((await s.fetch("/v1/nothing")).status).toBe(404);
  });
});

describe("withdrawing and listing", () => {
  it("withdraws one, then all, and lists what the key sent", async () => {
    const k = key(4);
    const a = await send(k, { steam_appid: 10 });
    await send(k, { steam_appid: 20, outcome: "stopped", stopped_for: ["grind"], factors: ["padding"], limited_time: "no" });
    await send(k, { steam_appid: 30 });
    await send(key(5), { steam_appid: 10 });

    const mine = (await (await s.fetch("/v1/verdicts/mine", get(k))).json()) as { verdicts: Record<string, unknown>[] };
    expect(mine.verdicts).toHaveLength(3);
    const twenty = mine.verdicts.find((v) => v.steam_appid === 20);
    expect(twenty).toMatchObject({ state: "live", outcome: "stopped", stopped_for: ["grind"], factors: ["padding"], licence: "CC0-1.0", consent: 1, vocab: 1, created_day: today() });
    expect(twenty).not.toHaveProperty("client");
    expect(twenty).not.toHaveProperty("client_version");

    expect((await s.fetch(`/v1/verdicts/${a.body.id}`, del(key(5)))).status).toBe(404);
    expect((await s.fetch(`/v1/verdicts/${a.body.id}`, del(k))).status).toBe(204);
    const all = await s.fetch("/v1/verdicts", del(k));
    expect(await all.json()).toEqual({ withdrawn: 2 });
    const states = await s.db.prepare("SELECT state, COUNT(*) AS n FROM verdicts GROUP BY state ORDER BY state").all();
    expect(states.results).toEqual([
      { state: "live", n: 1 },
      { state: "withdrawn", n: 3 },
    ]);
    // Sending again after withdrawing is a new verdict with a new id.
    const again = await send(k, { steam_appid: 10 });
    expect(again.status).toBe(201);
    expect(again.body.id).not.toBe(a.body.id);
  });
});

describe("limits", () => {
  it("allows 20 new verdicts a key a day, then answers 429 with Retry-After", async () => {
    const k = key(6);
    for (let i = 1; i <= 20; i++) expect((await send(k, { steam_appid: i })).status).toBe(201);
    const over = await send(k, { steam_appid: 21 });
    expect(over.status).toBe(429);
    const retry = Number(over.headers.get("retry-after"));
    expect(retry).toBeGreaterThan(0);
    expect(retry).toBeLessThanOrEqual(86_400);
  });

  it("allows 10 replacements a key a day", async () => {
    const k = key(7);
    await send(k);
    for (let i = 0; i < 10; i++) expect((await send(k, { worth: i % 2 ? "yes" : "no" })).status).toBe(200);
    expect((await send(k, { worth: "for_some" })).status).toBe(429);
  });

  it("has no first-week limit: a new key with many verdicts sends up to the daily limit", async () => {
    // A key with 200 verdicts from yesterday, all in what used to be its
    // first week.
    const k = key(8);
    const h = await sha256Hex(k);
    const rows = Array.from({ length: 200 }, (_, i) =>
      s.db
        .prepare("INSERT INTO verdicts (id, key_hash, steam_appid, outcome, worth, state, created_day, updated_day) VALUES (?, ?, ?, 'finished', 'yes', 'withdrawn', ?, ?)")
        .bind(`week${String(i).padStart(3, "0")}`, h, 100_000 + i, daysAgo(1), daysAgo(1)),
    );
    await s.db.batch(rows);
    for (let i = 0; i < 20; i++) expect((await send(k, { steam_appid: 900_000 + i })).status).toBe(201);
    const over = await send(k, { steam_appid: 900_100 });
    expect(over.status).toBe(429);
    expect(over.body.error).toBe("at most 20 new verdicts a day");
  });

  it("allows 60 writes a day from one address across keys", async () => {
    const ip = { "cf-connecting-ip": "192.168.0.42" };
    for (let i = 0; i < 60; i++) expect((await send(key(100 + i), {}, ip)).status).toBe(201);
    expect((await send(key(200), {}, ip)).status).toBe(429);
    expect((await send(key(200), {}, { "cf-connecting-ip": "192.168.0.43" })).status).toBe(201);
    // The address is never stored, only an HMAC under the day's salt.
    const counters = JSON.stringify((await s.db.prepare("SELECT * FROM counters").all()).results);
    expect(counters).not.toContain("192.168");
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM salts").first("n")).toBe(1);
  });

  it("counts an IPv6 client by its /64, whichever of its addresses it uses", async () => {
    // A device picks new addresses in its /64 as it likes, so 60 writes
    // from three of them fill the one limit.
    for (let i = 0; i < 60; i++) {
      const ip = { "cf-connecting-ip": `2001:db8:1:2::${(i % 3) + 1}` };
      expect((await send(key(300 + i), { steam_appid: 20_000 + i }, ip)).status).toBe(201);
    }
    const sameNetwork = await send(key(400), { steam_appid: 21_000 }, { "cf-connecting-ip": "2001:db8:1:2:a1b2:c3d4:e5f6:789" });
    expect(sameNetwork.status).toBe(429);
    expect(sameNetwork.body.error).toBe("too many verdicts and reports from this address today");
    expect((await send(key(401), { steam_appid: 21_001 }, { "cf-connecting-ip": "2001:db8:1:3::1" })).status).toBe(201);
    expect(JSON.stringify((await s.db.prepare("SELECT * FROM counters").all()).results)).not.toContain("2001");
  });
});

describe("parallel requests", () => {
  it("hold the per-key and per-address limits", async () => {
    const one = await Promise.all(Array.from({ length: 30 }, (_, i) => send(key(1000), { steam_appid: 10_000 + i })));
    expect(one.filter((r) => r.status === 201)).toHaveLength(20);
    expect(one.filter((r) => r.status === 429)).toHaveLength(10);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM verdicts").first("n")).toBe(20);

    await s.reset();
    const ip = { "cf-connecting-ip": "192.168.0.42" };
    const many = await Promise.all(Array.from({ length: 80 }, (_, i) => send(key(1100 + i), { steam_appid: 20_000 + i }, ip)));
    expect(many.filter((r) => r.status === 201)).toHaveLength(60);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM verdicts").first("n")).toBe(60);
    // A refused send leaves no key behind.
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM keys").first("n")).toBe(60);
  });

  it("make one verdict for one key's game, and replace it", async () => {
    const k = key(1200);
    const all = await Promise.all(Array.from({ length: 6 }, (_, i) => send(k, { steam_appid: 30, worth: i % 2 ? "yes" : "no" })));
    expect(all.filter((r) => r.status === 201)).toHaveLength(1);
    expect(all.filter((r) => r.status === 200)).toHaveLength(5);
    expect(new Set(all.map((r) => r.body.id)).size).toBe(1);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM verdicts").first("n")).toBe(1);
  });

  it("count a key's report once", async () => {
    const v = await send(key(1300), { text: "Clicks after the first boss." });
    await publish();
    const body = { verdict_id: v.body.id, reason: "spam" };
    const all = await Promise.all(Array.from({ length: 5 }, () => s.fetch("/v1/reports", post(key(1301), body))));
    expect(all.map((r) => r.status)).toEqual([202, 202, 202, 202, 202]);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(1);
    expect(await s.db.prepare("SELECT n FROM counters WHERE name LIKE 'key:%:reports'").first("n")).toBe(1);
  });
});

describe("keys", () => {
  it("are written only with a key's first verdict", async () => {
    await s.fetch("/v1/verdicts", post(key(1400), "{not json"));
    await s.fetch("/v1/verdicts", post(key(1401), verdict({ licence: "CC-BY-4.0" })));
    const v = await send(key(1402));
    await publish();
    expect((await s.fetch("/v1/reports", post(key(1403), { verdict_id: v.body.id, reason: "spam" }))).status).toBe(202);
    expect((await s.fetch("/v1/reports", post(key(1404), { verdict_id: "a".repeat(26), reason: "spam" }))).status).toBe(404);
    const keys = await s.db.prepare("SELECT key_hash FROM keys").all<{ key_hash: string }>();
    expect(keys.results.map((r) => r.key_hash)).toEqual([await sha256Hex(key(1402))]);
  });
});

describe("brigading", () => {
  it("holds a game's new verdicts past 20 in a day, and a held verdict stays held when replaced", async () => {
    for (let i = 0; i < 20; i++) expect((await send(key(300 + i), { steam_appid: 999 })).body.state).toBe("live");
    const held = await send(key(400), { steam_appid: 999 });
    expect(held.status).toBe(201);
    expect(held.body.state).toBe("held");
    const replaced = await send(key(400), { steam_appid: 999, worth: "no" });
    expect(replaced.body).toMatchObject({ state: "held", replaced: true });
    // Another game isn't affected.
    expect((await send(key(400), { steam_appid: 1000 })).body.state).toBe("live");
  });

  it("holds at ten times a game's usual rate", async () => {
    // One verdict a week for the last four weeks: a usual rate of 1/7 a day.
    for (const [i, d] of [7, 14, 21, 28].entries()) {
      await s.db.prepare("INSERT INTO keys (key_hash) VALUES (?)").bind(`x${i}`).run();
      await s.db
        .prepare("INSERT INTO verdicts (id, key_hash, steam_appid, outcome, worth, created_day, updated_day) VALUES (?, ?, 77, 'finished', 'yes', ?, ?)")
        .bind(`old${i}`.padEnd(26, "a"), `x${i}`, daysAgo(d), daysAgo(d))
        .run();
    }
    const states = [];
    for (let i = 0; i < 6; i++) states.push((await send(key(500 + i), { steam_appid: 77 })).body.state);
    // Ten times 4/28 is under 2, but the floor of 5 a day comes first.
    expect(states).toEqual(["live", "live", "live", "live", "held", "held"]);
  });
});

describe("reports", () => {
  it("counts each key once, holds a text at three keys and only takes published verdicts", async () => {
    const v = await send(key(600), { text: "Clicks after the first boss." });
    const id = v.body.id as string;
    // The reporters have published verdicts of their own.
    for (const n of [601, 602, 603]) await send(key(n), { steam_appid: n });
    const report = (k: string, body: unknown = { verdict_id: id, reason: "spam" }) => s.fetch("/v1/reports", post(k, body));
    expect((await report(key(601))).status).toBe(404);
    await publish();
    expect((await report(key(601), { verdict_id: id, reason: "rude" })).status).toBe(400);
    expect((await report(key(601), { verdict_id: id, reason: "spam", note: "x" })).status).toBe(400);
    expect((await report(key(601))).status).toBe(202);
    expect((await report(key(601))).status).toBe(202);
    expect((await report(key(602), { verdict_id: id, reason: "personal" })).status).toBe(202);
    expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe("shown");
    expect((await report(key(603))).status).toBe(202);
    expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe("held");
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(3);
  });

  it("holds a text only on reports from keys with a published verdict of their own", async () => {
    const v = await send(key(610), { text: "A fair, ordinary review." });
    const id = v.body.id as string;
    await send(key(611), { steam_appid: 611 });
    await publish();
    const report = (k: string) => s.fetch("/v1/reports", post(k, { verdict_id: id, reason: "spam" }));
    const textState = () => s.db.prepare("SELECT text_state FROM verdicts WHERE id = ?").bind(id).first("text_state");
    // Keys cost nothing to make: three new ones are counted, but don't hold it.
    for (const n of [620, 621, 622]) expect((await report(key(n))).status).toBe(202);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(3);
    expect(await textState()).toBe("shown");
    // Nor does a key whose verdicts aren't published yet, or were withdrawn.
    await send(key(623), { steam_appid: 623 });
    await send(key(624), { steam_appid: 624 });
    await publish();
    await s.db.prepare("UPDATE verdicts SET state = 'withdrawn' WHERE steam_appid = 624").run();
    await send(key(625), { steam_appid: 625 });
    for (const n of [624, 625]) expect((await report(key(n))).status).toBe(202);
    expect(await textState()).toBe("shown");
    // Two keys with published verdicts don't, and a third does.
    expect((await report(key(611))).status).toBe(202);
    expect((await report(key(623))).status).toBe(202);
    expect(await textState()).toBe("shown");
    await send(key(626), { steam_appid: 626 });
    await publish();
    expect((await report(key(626))).status).toBe(202);
    expect(await textState()).toBe("held");
  });

  it("allows 20 reports a key a day", async () => {
    const ids = [];
    for (let i = 0; i < 21; i++) ids.push((await send(key(700 + i), { steam_appid: 5000 + i, text: `Verdict ${i}.` })).body.id as string);
    await publish();
    const k = key(800);
    for (let i = 0; i < 20; i++) expect((await s.fetch("/v1/reports", post(k, { verdict_id: ids[i], reason: "off_topic" }))).status).toBe(202);
    expect((await s.fetch("/v1/reports", post(k, { verdict_id: ids[20], reason: "off_topic" }))).status).toBe(429);
  });
});

describe("blocked keys", () => {
  it("refuses a blocked key's sends and reports but lets it withdraw", async () => {
    const k = key(900);
    const v = await send(k);
    await s.db.prepare("UPDATE keys SET blocked = 1").run();
    expect((await send(k, { steam_appid: 5 })).status).toBe(403);
    expect((await s.fetch("/v1/reports", post(k, { verdict_id: v.body.id, reason: "spam" }))).status).toBe(403);
    expect((await s.fetch(`/v1/verdicts/${v.body.id}`, del(k))).status).toBe(204);
  });
});

describe("pages", () => {
  it("serves the terms with the consent text and no scripts", async () => {
    const r = await s.fetch("/terms");
    expect(r.status).toBe(200);
    const html = await r.text();
    expect(html).toContain("Your verdict will be public.");
    expect(html).toContain("It&#39;s left out of the next daily update and gone from UpNext&#39;s servers within a week");
    expect(html).toContain("CC0 1.0");
    expect(html).toContain('runs on Cloudflare (Workers, D1 and R2)');
    expect(html).toContain('href="https://www.cloudflare.com/privacypolicy/"');
    expect(html).toContain("no support and no warranty of any kind");
    expect(html).not.toContain("can't be revoked");
    expect(html).not.toContain("<script");
    expect(r.headers.get("content-security-policy")).toContain("default-src 'none'");
  });
});

describe("the client a limit counts", () => {
  it("is an IPv4 address as it is and an IPv6 address by its /64", () => {
    expect(clientNetwork("192.168.0.42")).toBe("192.168.0.42");
    expect(clientNetwork("::ffff:192.168.0.42")).toBe("192.168.0.42");
    expect(clientNetwork("::FFFF:c0a8:2a")).toBe("192.168.0.42");
    for (const a of ["2001:db8:1:2::1", "2001:0db8:0001:0002:ffff:ffff:ffff:ffff", "2001:DB8:1:2:a1b2:c3d4:e5f6:789", "2001:db8:1:2::1.2.3.4"]) {
      expect(clientNetwork(a)).toBe("2001:db8:1:2::/64");
    }
    expect(clientNetwork("2001:db8::1")).toBe("2001:db8:0:0::/64");
    expect(clientNetwork("::1")).toBe("0:0:0:0::/64");
    expect(clientNetwork("fe80::1%eth0")).toBe("fe80:0:0:0::/64");
    // Anything else is counted as it's written.
    for (const a of ["unknown", "2001:db8:::1", "2001:db8:1:2:3:4:5:6:7", "1:2:3:4:5:6:7:1.2.3.4", "::1.2.3.400", "g::1"]) {
      expect(clientNetwork(a)).toBe(a);
    }
  });
});
