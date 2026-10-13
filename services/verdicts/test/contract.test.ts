// The service checks what UpNext checks: the contract's own patterns, the
// app's text-check cases, the same problems for the same verdicts, and a
// summary byte for byte like the one UpNext's writer made for the fixture.

import { readFileSync } from "node:fs";
import { join } from "node:path";
import { gunzipSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { CONTRACT, LIFE_REASONS, NOTICE, STOP_REASONS, TEXT_CHECKS } from "../src/contract";
import * as L from "../src/limits";
import { type Published, summarise, writeSnapshot } from "../src/snapshot";
import { base32, gunzip, isVerdictId, newVerdictId } from "../src/util";
import { checkTyped, readVerdict, textRefusal } from "../src/verdict";
import { REPO, verdict } from "./harness";

const FIXTURES = join(REPO, "tests/fixtures/verdicts");

function lines(file: string): string[] {
  return gunzipSync(readFileSync(join(FIXTURES, file))).toString("utf8").split("\n").filter((l) => l !== "");
}

describe("the contract", () => {
  it("runs the contract's pattern strings unchanged", () => {
    for (const c of TEXT_CHECKS) expect(c.re.source.replace(/\\\//g, "/")).toBe(c.pattern);
    expect(TEXT_CHECKS.map((c) => [c.code, c.pattern, c.re.flags])).toEqual(CONTRACT.text_checks.map((c) => [c.code, c.pattern, `${c.flags}u`]));
  });

  it("refuses and accepts the app's text-check cases", () => {
    const cases = JSON.parse(readFileSync(join(FIXTURES, "text-checks.json"), "utf8")) as { text: string; refused: string | null }[];
    expect(cases.length).toBeGreaterThan(20);
    for (const c of cases) expect([c.text, textRefusal(c.text)?.code ?? null]).toEqual([c.text, c.refused]);
  });

  it("writes the app's notice and never accepts a life reason", () => {
    const manifest = JSON.parse(readFileSync(join(FIXTURES, "verdicts.json"), "utf8")) as { notice: string };
    expect(NOTICE).toBe(manifest.notice);
    for (const r of LIFE_REASONS) expect(STOP_REASONS.has(r)).toBe(false);
  });

  it("enforces the limits docs/VERDICTS.md states", () => {
    const docs = readFileSync(join(REPO, "docs/VERDICTS.md"), "utf8");
    expect(docs).toContain(`| New verdicts per key | ${L.NEW_PER_DAY} a day |`);
    expect(docs).toContain(`| Replacements per key | ${L.REPLACED_PER_DAY} a day |`);
    expect(docs).toContain(`| Reports per key | ${L.REPORTS_PER_DAY} a day |`);
    expect(docs).toContain(`| Writes per client address | ${L.ADDRESS_WRITES_PER_DAY} a day across all keys.`);
    expect(docs).toContain(`at most ${L.BODY_MAX / 1024} KB`);
  });
});

describe("a verdict", () => {
  it("is accepted as UpNext sends it, with null read as left out", () => {
    expect(readVerdict(verdict({ clicked: null, factors: [], text: "Clicks in the first hour." })).problems).toEqual([]);
  });

  it("is refused with an unknown field, a wrong type or a missing version", () => {
    expect(readVerdict(verdict({ steam_id: "76561197960287930" })).problems).toEqual([{ field: "steam_id", why: "isn't a field of a verdict" }]);
    expect(readVerdict(verdict({ steam_appid: "620" })).problems).toEqual([{ field: "steam_appid", why: "isn't a Steam appid" }]);
    expect(readVerdict(verdict({ steam_appid: -1 })).problems[0].field).toBe("steam_appid");
    expect(readVerdict(verdict({ factors: "padding" })).problems).toEqual([{ field: "factors", why: "isn't a list of strings" }]);
    const { client: _, ...noClient } = verdict();
    expect(readVerdict(noClient).problems).toEqual([{ field: "client", why: "is required" }]);
    expect(readVerdict([verdict()]).problems[0].field).toBe("body");
  });

  it("names every wrong answer the way UpNext does", () => {
    const { problems } = readVerdict(
      verdict({
        steam_appid: undefined,
        gog_id: "12a",
        outcome: undefined,
        worth: "maybe",
        factors: ["padding", "padding", "save_anywhere", "recap", "teleport"],
        stop_note: "after chapter 3",
        stopped_for: ["grind", "busy"],
        text: "x".repeat(501),
        vocab: 2,
        licence: "CC-BY-4.0",
        consent: 0,
        client: "0.1.0 beta",
      }),
    );
    expect(problems).toEqual([
      { field: "gog_id", why: "isn't a GOG id: digits only" },
      { field: "outcome", why: "is required" },
      { field: "worth", why: '"maybe" isn\'t one of its answers' },
      { field: "factors", why: "go with an answer to whether it respects limited time" },
      { field: "factors", why: "at most 3" },
      { field: "factors", why: '"teleport" isn\'t one of them' },
      { field: "factors", why: "each at most once" },
      { field: "stop_note", why: "goes with an answer to when it's fine to stop" },
      { field: "stopped_for", why: '"busy" is about you, not the game, and is never sent' },
      { field: "text", why: "is 501 characters; at most 500" },
      { field: "vocab", why: "2 isn't a vocabulary this build knows (1 to 1)" },
      { field: "licence", why: "verdicts are published under CC0-1.0 only" },
      { field: "consent", why: "consent version 0 isn't accepted (1 to 1)" },
      { field: "client", why: "isn't an UpNext version" },
    ]);
  });

  it("needs a game, keeps reasons to stopped games and checks the typed text", () => {
    expect(readVerdict(verdict({ steam_appid: undefined })).problems).toEqual([{ field: "game", why: "a verdict needs the game's Steam appid or GOG id" }]);
    expect(readVerdict(verdict({ steam_appid: 0 })).problems).toEqual([{ field: "steam_appid", why: "isn't a Steam appid" }]);
    expect(readVerdict(verdict({ stopped_for: ["grind"] })).problems).toEqual([{ field: "stopped_for", why: "is only for games shelved, stopped or bounced" }]);
    expect(readVerdict(verdict({ outcome: "stopped", stopped_for: ["grind"] })).problems).toEqual([]);
    expect(checkTyped("   ", 500)).toBe("is empty; leave it out instead");
    expect(checkTyped("mail someone@example.org", 500)).toBe("has an email address; take it out to send");
    // Characters, not UTF-16 units: 500 emoji are 1,000 units and still fit.
    expect(checkTyped("🎮".repeat(500), 500)).toBeUndefined();
    expect(checkTyped("🎮".repeat(501), 500)).toBe("is 501 characters; at most 500");
  });
});

describe("ids", () => {
  it("are 26 characters of lowercase base32", () => {
    expect(base32(new Uint8Array([0xff, 0, 0x10]))).toBe("74aba");
    const ids = new Set(Array.from({ length: 200 }, newVerdictId));
    expect(ids.size).toBe(200);
    for (const id of ids) expect(isVerdictId(id)).toBe(true);
  });
});

// The Rust fixture's full file as the service's `Published` verdicts.
function fixtureVerdicts(): Published[] {
  return lines("verdicts-all-2026-10-09.jsonl.gz")
    .slice(1)
    .map((l) => {
      const v = JSON.parse(l) as Published;
      return { ...v, factors: v.factors ?? [], stopped_for: v.stopped_for ?? [] };
    });
}

describe("the snapshot", () => {
  it("sums up the fixture's verdicts exactly as UpNext's summarise does", () => {
    const want = lines("verdicts-summary-2026-10-09.jsonl.gz").slice(1);
    const got = summarise(fixtureVerdicts()).map((g) => JSON.stringify(g.texts.length > 0 ? g : { ...g, texts: undefined }));
    expect(got).toEqual(want);
  });

  it("writes the fixture's summary byte for byte before gzip", async () => {
    const snap = await writeSnapshot("2026-10-09", fixtureVerdicts());
    const fixture = lines("verdicts-summary-2026-10-09.jsonl.gz");
    const written = new TextDecoder().decode(await gunzip(snap.summary)).split("\n").filter((l) => l !== "");
    expect(written).toEqual(fixture);
    const manifest = JSON.parse(readFileSync(join(FIXTURES, "verdicts.json"), "utf8")) as Record<string, unknown>;
    const { summary: _s, all: _a, ...rest } = manifest;
    const { summary, all, ...ours } = snap.manifest;
    expect(ours).toEqual(rest);
    expect([summary.file, all.file]).toEqual(["verdicts-summary-2026-10-09.jsonl.gz", "verdicts-all-2026-10-09.jsonl.gz"]);
    expect(summary.bytes).toBe(snap.summary.length);
  });

  it("writes every verdict in the full file, with the same lines UpNext writes", async () => {
    const fixture = lines("verdicts-all-2026-10-09.jsonl.gz");
    const snap = await writeSnapshot("2026-10-09", fixtureVerdicts());
    const written = new TextDecoder().decode(await gunzip(snap.all)).split("\n").filter((l) => l !== "");
    // The fixture's header checksums its lines in its own order; the
    // service writes them in id order, so the lines match as a set.
    expect(written.slice(1).sort()).toEqual(fixture.slice(1).sort());
    const head = JSON.parse(written[0]) as Record<string, unknown>;
    expect(head).toMatchObject({ upnext_verdicts: 1, kind: "all", date: "2026-10-09", vocab: 1, count: 37, verdicts: 37, licence: "CC0-1.0" });
  });
});
