// Cloudflare Access in front of `/admin`. Access signs in moderators (with
// GitHub or a one-time email code) and adds a signed token to every request
// it lets through; the Worker checks that token itself, so a missing or
// misconfigured Access application leaves the admin pages closed rather
// than open. Cloudflare's guide: "Validate JWTs" in the Cloudflare One docs.

import type { Env } from "./env";

interface Jwk extends JsonWebKey {
  kid?: string;
}

// The tests' own signing keys count only for a team under `.example`, a
// name reserved for examples that can never be a real Access team, so
// `ACCESS_TEST_JWKS` set by mistake on a deployed copy is ignored there.
function testing(team: string): boolean {
  try {
    return new URL(team).hostname.endsWith(".example");
  } catch {
    return false;
  }
}

let cachedKeys: { at: number; team: string; keys: Jwk[] } | undefined;

// Access rotates its keys every six weeks and keeps old ones valid for a
// week, so an hour's cache is safe.
async function signingKeys(env: Env, team: string): Promise<Jwk[]> {
  if (env.ACCESS_TEST_JWKS && testing(team)) return (JSON.parse(env.ACCESS_TEST_JWKS) as { keys: Jwk[] }).keys;
  if (cachedKeys && cachedKeys.team === team && Date.now() - cachedKeys.at < 3_600_000) return cachedKeys.keys;
  const r = await fetch(`${team}/cdn-cgi/access/certs`);
  if (!r.ok) throw new Error(`Access's keys answered ${r.status}`);
  const keys = ((await r.json()) as { keys: Jwk[] }).keys;
  cachedKeys = { at: Date.now(), team, keys };
  return keys;
}

function b64url(s: string): Uint8Array {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/").padEnd(Math.ceil(s.length / 4) * 4, "="));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

function part(s: string): Record<string, unknown> {
  return JSON.parse(new TextDecoder().decode(b64url(s))) as Record<string, unknown>;
}

// The signed-in moderator's email, or why the request isn't let in.
export async function moderator(req: Request, env: Env): Promise<{ email: string } | { refused: string }> {
  const team = env.ACCESS_TEAM_DOMAIN?.replace(/\/+$/, "");
  const aud = env.ACCESS_AUD;
  if (!team || !aud) return { refused: "Access isn't set up for this service" };
  const token = req.headers.get("cf-access-jwt-assertion");
  if (!token) return { refused: "no Access token" };
  const [h, p, s] = token.split(".");
  if (!h || !p || !s) return { refused: "not an Access token" };
  try {
    const header = part(h);
    if (header.alg !== "RS256") return { refused: "not an Access token" };
    const jwk = (await signingKeys(env, team)).find((k) => k.kid === header.kid);
    if (!jwk) return { refused: "signed with a key Access doesn't list" };
    const key = await crypto.subtle.importKey("jwk", jwk, { name: "RSASSA-PKCS1-v1_5", hash: "SHA-256" }, false, ["verify"]);
    const ok = await crypto.subtle.verify("RSASSA-PKCS1-v1_5", key, b64url(s), new TextEncoder().encode(`${h}.${p}`));
    if (!ok) return { refused: "the Access token's signature doesn't match" };
    const claims = part(p);
    const auds = Array.isArray(claims.aud) ? claims.aud : [claims.aud];
    if (!auds.includes(aud)) return { refused: "the Access token is for another application" };
    if (claims.iss !== team) return { refused: "the Access token is from another team" };
    const now = Date.now() / 1000;
    if (typeof claims.exp !== "number" || claims.exp < now) return { refused: "the Access token has expired" };
    if (typeof claims.nbf === "number" && claims.nbf > now + 60) return { refused: "the Access token isn't valid yet" };
    if (typeof claims.email !== "string" || claims.email === "") return { refused: "the Access token names nobody" };
    return { email: claims.email };
  } catch {
    return { refused: "not an Access token" };
  }
}
