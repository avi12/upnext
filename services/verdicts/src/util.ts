// Small helpers: days, ids, hashes and gzip, all with the platform's own
// Web Crypto and compression streams, so they run the same in a Worker and in
// the tests' Node.

export function dayOf(ms: number): string {
  return new Date(ms).toISOString().slice(0, 10);
}

export function today(): string {
  return dayOf(Date.now());
}

// The day `n` days after `day` (before it when `n` is negative).
export function addDays(day: string, n: number): string {
  return dayOf(Date.parse(`${day}T00:00:00Z`) + n * 86_400_000);
}

// Seconds until the next UTC midnight, for `Retry-After` on a day's limit.
export function secondsToMidnight(now = Date.now()): number {
  const next = Date.parse(`${addDays(dayOf(now), 1)}T00:00:00Z`);
  return Math.max(1, Math.ceil((next - now) / 1000));
}

export function hex(bytes: ArrayBuffer | Uint8Array): string {
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

// The casts say what's true of every array here: none is backed by a
// SharedArrayBuffer, which the DOM's typings for Web Crypto and Blob refuse.
type Bytes = Uint8Array<ArrayBuffer>;

export async function sha256Hex(data: string | Uint8Array): Promise<string> {
  const bytes = typeof data === "string" ? new TextEncoder().encode(data) : data;
  return hex(await crypto.subtle.digest("SHA-256", bytes as Bytes));
}

export async function hmacHex(key: string, data: string): Promise<string> {
  const k = await crypto.subtle.importKey("raw", new TextEncoder().encode(key), { name: "HMAC", hash: "SHA-256" }, false, ["sign"]);
  return hex(await crypto.subtle.sign("HMAC", k, new TextEncoder().encode(data)));
}

// The client a per-address limit counts: an IPv4 address as it is, and an
// IPv6 address by its /64, the smallest network a subscriber is normally
// given. A device picks new addresses in its /64 as it likes, so counted
// one by one, a single client would have as many limits as it wanted. An
// IPv4 address written as IPv6 (`::ffff:192.0.2.1`) is the IPv4 address,
// and anything that isn't an address is counted as it's written.
export function clientNetwork(address: string): string {
  if (!address.includes(":")) return address;
  const g = ipv6Groups(address.toLowerCase());
  if (!g) return address;
  if (g.slice(0, 5).every((x) => x === 0) && g[5] === 0xffff) {
    return [g[6] >> 8, g[6] & 255, g[7] >> 8, g[7] & 255].join(".");
  }
  return `${g
    .slice(0, 4)
    .map((x) => x.toString(16))
    .join(":")}::/64`;
}

// An IPv6 address's eight 16-bit groups, or null if it isn't one.
function ipv6Groups(text: string): number[] | null {
  let a = text.split("%")[0];
  // A last part written as IPv4 is the last two groups.
  const tail: number[] = [];
  const v4 = /(?:^|:)(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3})$/.exec(a);
  if (v4) {
    const b = v4.slice(1).map(Number);
    if (b.some((x) => x > 255)) return null;
    tail.push((b[0] << 8) | b[1], (b[2] << 8) | b[3]);
    a = a.slice(0, a.length - v4[0].length + (v4[0].startsWith(":") ? 1 : 0));
    if (a.endsWith(":") && !a.endsWith("::")) a = a.slice(0, -1);
  }
  const halves = a.split("::");
  if (halves.length > 2) return null;
  const parse = (part: string) => (part === "" ? [] : part.split(":").map((h) => (/^[0-9a-f]{1,4}$/.test(h) ? parseInt(h, 16) : Number.NaN)));
  const head = parse(halves[0]);
  const rest = halves.length === 2 ? parse(halves[1]) : [];
  const want = 8 - tail.length;
  let groups: number[];
  if (halves.length === 2) {
    const zeros = want - head.length - rest.length;
    if (zeros < 1) return null;
    groups = [...head, ...Array.from({ length: zeros }, () => 0), ...rest];
  } else {
    if (head.length !== want) return null;
    groups = head;
  }
  if (groups.some(Number.isNaN)) return null;
  return [...groups, ...tail];
}

const BASE32 = "abcdefghijklmnopqrstuvwxyz234567";

// Lowercase base32 without padding (RFC 4648's alphabet, lowercased).
export function base32(bytes: Uint8Array): string {
  let out = "";
  let bits = 0;
  let value = 0;
  for (const b of bytes) {
    value = ((value << 8) | b) & 0xffff;
    bits += 8;
    while (bits >= 5) {
      out += BASE32[(value >>> (bits - 5)) & 31];
      bits -= 5;
    }
  }
  if (bits > 0) out += BASE32[(value << (5 - bits)) & 31];
  return out;
}

// A verdict's id: 16 random bytes, 26 characters of base32.
export function newVerdictId(): string {
  return base32(crypto.getRandomValues(new Uint8Array(16)));
}

export function isVerdictId(s: string): boolean {
  return /^[a-z2-7]{26}$/.test(s);
}

export async function gzip(data: Uint8Array): Promise<Bytes> {
  const stream = new Blob([data as Bytes]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

export async function gunzip(data: Uint8Array): Promise<Bytes> {
  const stream = new Blob([data as Bytes]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}
