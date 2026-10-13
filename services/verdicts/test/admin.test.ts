// The moderators' pages behind Cloudflare Access: closed without a valid
// Access token, and each action applied and logged. The tests sign their own
// tokens with a key pair made here, passed to the Worker as
// `ACCESS_TEST_JWKS`, so nothing reaches Cloudflare.

import { afterAll, beforeAll, beforeEach, describe, expect, it } from "vitest";
import { type Service, daysAgo, key, post, start, today, verdict } from "./harness";

// A made-up team address; the tests never fetch it.
const TEAM = "https://team.example";
const AUD = "test-audience-tag";

let s: Service;
let signer: CryptoKey;
let other: CryptoKey;
let jwks: string;

function b64url(data: Uint8Array | string): string {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  return btoa(String.fromCharCode(...bytes)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

async function token(claims: Record<string, unknown> = {}, with_: CryptoKey = signer, kid = "k1"): Promise<string> {
  const head = b64url(JSON.stringify({ alg: "RS256", kid, typ: "JWT" }));
  const body = b64url(JSON.stringify({ aud: [AUD], iss: TEAM, email: "moderator@example.org", exp: Math.floor(Date.now() / 1000) + 600, ...claims }));
  const sig = new Uint8Array(await crypto.subtle.sign("RSASSA-PKCS1-v1_5", with_, new TextEncoder().encode(`${head}.${body}`)));
  return `${head}.${body}.${b64url(sig)}`;
}

async function pair(): Promise<CryptoKeyPair> {
  return crypto.subtle.generateKey({ name: "RSASSA-PKCS1-v1_5", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }, true, [
    "sign",
    "verify",
  ]) as Promise<CryptoKeyPair>;
}

beforeAll(async () => {
  const a = await pair();
  const b = await pair();
  signer = a.privateKey;
  other = b.privateKey;
  const jwk = { ...(await crypto.subtle.exportKey("jwk", a.publicKey)), kid: "k1", alg: "RS256", use: "sig" };
  jwks = JSON.stringify({ keys: [jwk] });
  s = await start({ ACCESS_TEAM_DOMAIN: TEAM, ACCESS_AUD: AUD, ACCESS_TEST_JWKS: jwks });
});
afterAll(async () => {
  await s.dispose();
});
beforeEach(async () => {
  await s.reset();
});

async function as(t: string, path = "/admin", init: RequestInit = {}) {
  return s.fetch(path, { ...init, headers: { "cf-access-jwt-assertion": t, ...(init.headers as Record<string, string>) } });
}

async function act(action: string, id: string, origin = "https://verdicts.test") {
  const form = new URLSearchParams({ action, id, note: "checked" });
  return as(await token(), "/admin/act", {
    method: "POST",
    body: form.toString(),
    headers: { "content-type": "application/x-www-form-urlencoded", origin },
    redirect: "manual",
  });
}

describe("Access", () => {
  it("keeps the admin pages closed without a valid token", async () => {
    expect((await s.fetch("/admin")).status).toBe(404);
    expect((await as("not.a.token")).status).toBe(404);
    expect((await as(await token({}, other))).status).toBe(404);
    expect((await as(await token({}, signer, "k2"))).status).toBe(404);
    expect((await as(await token({ aud: ["another-app"] }))).status).toBe(404);
    expect((await as(await token({ iss: "https://other-team.example" }))).status).toBe(404);
    expect((await as(await token({ exp: Math.floor(Date.now() / 1000) - 10 }))).status).toBe(404);
    const ok = await as(await token());
    expect(ok.status).toBe(200);
    expect(ok.headers.get("cache-control")).toBe("no-store");
    expect(await ok.text()).toContain("Signed in as moderator@example.org");
    expect(s.outbound).toEqual([]);
  });

  it("ignores the test keys for a real team's address", async () => {
    // The Worker would fetch this team's keys and fail; it must not take
    // the test keys instead.
    const real = await start({ ACCESS_TEAM_DOMAIN: "https://some-other-team.cloudflareaccess.com", ACCESS_AUD: AUD, ACCESS_TEST_JWKS: jwks });
    try {
      const t = await token({ iss: "https://some-other-team.cloudflareaccess.com" });
      expect((await real.fetch("/admin", { headers: { "cf-access-jwt-assertion": t } })).status).toBe(404);
      expect(real.outbound).toEqual(["https://some-other-team.cloudflareaccess.com/cdn-cgi/access/certs"]);
    } finally {
      await real.dispose();
    }
  });

  it("stays closed when Access isn't configured", async () => {
    const bare = await start();
    try {
      expect((await bare.fetch("/admin", { headers: { "cf-access-jwt-assertion": await token() } })).status).toBe(404);
    } finally {
      await bare.dispose();
    }
  });
});

describe("moderating", () => {
  it("lists reported texts escaped, hides and shows them, and logs each action", async () => {
    const sent = (await (await s.fetch("/v1/verdicts", post(key(1), verdict({ text: "<b>bold</b> & loud" })))).json()) as { id: string };
    await s.db.prepare("UPDATE verdicts SET text_state = 'held'").run();
    const page = await (await as(await token())).text();
    expect(page).toContain("&lt;b&gt;bold&lt;/b&gt; &amp; loud");
    expect(page).not.toContain("<b>bold</b>");
    expect(page).not.toContain("key_hash");

    const r = await act("hide_text", sent.id);
    expect(r.status).toBe(303);
    expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe("hidden");
    await act("show_text", sent.id);
    expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe("shown");

    const log = await s.db.prepare("SELECT at, moderator, action, verdict_id, note FROM moderation_log ORDER BY rowid").all();
    expect(log.results).toEqual([
      { at: today(), moderator: "moderator@example.org", action: "hide_text", verdict_id: sent.id, note: "checked" },
      { at: today(), moderator: "moderator@example.org", action: "show_text", verdict_id: sent.id, note: "checked" },
    ]);
    expect(await (await as(await token(), "/admin/log")).text()).toContain("hide_text");
  });

  it("lists a reported text that isn't held, and dismisses its reports", async () => {
    const sent = (await (await s.fetch("/v1/verdicts", post(key(1), verdict({ text: "Reported once, still shown." })))).json()) as { id: string };
    // Sent days ago, so it isn't among the new texts, and reported once.
    await s.db.batch([
      s.db.prepare("UPDATE verdicts SET updated_day = ?, published_day = ?").bind(daysAgo(3), daysAgo(3)),
      s.db.prepare("INSERT INTO reports (verdict_id, key_hash, reason, day) VALUES (?, 'r', 'spam', ?)").bind(sent.id, today()),
    ]);
    const reported = async () => {
      const page = await (await as(await token())).text();
      return page.slice(page.indexOf("<h2>Reported texts</h2>"), page.indexOf("<h2>Held verdicts</h2>"));
    };
    const before = await reported();
    expect(before).toContain("Reported once, still shown.");
    expect(before).toContain("Reports: spam");
    expect(before).toContain('value="hide_text"');
    expect(before).toContain('value="dismiss_reports"');

    expect((await act("dismiss_reports", sent.id)).status).toBe(303);
    expect(await s.db.prepare("SELECT COUNT(*) AS n FROM reports").first("n")).toBe(0);
    expect(await s.db.prepare("SELECT text_state FROM verdicts").first("text_state")).toBe("shown");
    expect(await reported()).toContain("No text is waiting.");
    expect(await s.db.prepare("SELECT action FROM moderation_log").first("action")).toBe("dismiss_reports");
  });

  it("removes a verdict, releases a held game and blocks a key", async () => {
    const a = (await (await s.fetch("/v1/verdicts", post(key(1), verdict()))).json()) as { id: string };
    const b = (await (await s.fetch("/v1/verdicts", post(key(2), verdict()))).json()) as { id: string };
    const c = (await (await s.fetch("/v1/verdicts", post(key(3), verdict()))).json()) as { id: string };
    await s.db.prepare("UPDATE verdicts SET state = 'held' WHERE id IN (?, ?)").bind(b.id, c.id).run();
    await act("remove", a.id);
    await act("release_game", b.id);
    const states = await s.db.prepare("SELECT id, state FROM verdicts").all<{ id: string; state: string }>();
    expect(Object.fromEntries(states.results.map((r) => [r.id, r.state]))).toEqual({ [a.id]: "removed", [b.id]: "live", [c.id]: "live" });

    await act("block", b.id);
    expect((await s.fetch("/v1/verdicts", post(key(2), verdict({ steam_appid: 9 })))).status).toBe(403);
    await act("unblock", b.id);
    expect((await s.fetch("/v1/verdicts", post(key(2), verdict({ steam_appid: 9 })))).status).toBe(201);
    const logged = await s.db.prepare("SELECT action, key_hash IS NOT NULL AS keyed FROM moderation_log ORDER BY rowid").all();
    expect(logged.results).toEqual([
      { action: "remove", keyed: 0 },
      { action: "release_game", keyed: 0 },
      { action: "block", keyed: 1 },
      { action: "unblock", keyed: 1 },
    ]);
  });

  it("refuses a form posted from another site", async () => {
    const a = (await (await s.fetch("/v1/verdicts", post(key(1), verdict()))).json()) as { id: string };
    expect((await act("remove", a.id, "https://evil.example")).status).toBe(403);
    expect(await s.db.prepare("SELECT state FROM verdicts").first("state")).toBe("live");
  });
});
