import { afterEach, describe, expect, it } from "vitest";
import { bookmarklet, codeFromHash, keyFromHash, saveSignIn, takeSignIn } from "./gogLink";

describe("the Send to UpNext bookmark", () => {
  it("sends the code from GOG's page to this UpNext's callback page, with the key", () => {
    const href = bookmarklet("http://localhost:8787", "k3y");
    expect(href.startsWith("javascript:")).toBe(true);
    const code = decodeURIComponent(href.slice("javascript:".length));
    let went = "";
    const location = { search: "?origin=client&code=abc-123", set href(v: string) { went = v; } };
    new Function("location", "alert", code)(location, () => { throw new Error("no alert expected"); });
    expect(went).toBe("http://localhost:8787/gog/callback#key=k3y&code=abc-123");
    expect(keyFromHash(new URL(went).hash)).toBe("k3y");
    expect(codeFromHash(new URL(went).hash)).toBe("abc-123");
  });

  it("says where to use it on any other page", () => {
    const code = decodeURIComponent(bookmarklet("http://localhost:8787", "k3y").slice("javascript:".length));
    const said: string[] = [];
    const location = { search: "?q=1", set href(_: string) { throw new Error("no navigation expected"); } };
    new Function("location", "alert", code)(location, (m: string) => said.push(m));
    expect(said[0]).toContain("GOG’s page after you sign in");
  });

  it("reads the code from the fragment", () => {
    expect(codeFromHash("#code=abc-123")).toBe("abc-123");
    expect(codeFromHash("")).toBeNull();
    expect(codeFromHash("#code=")).toBeNull();
    expect(keyFromHash("#code=abc-123")).toBeNull();
  });
});

describe("the sign-in this browser started", () => {
  afterEach(() => localStorage.clear());

  it("is taken once, and not after it expires", () => {
    saveSignIn({ state: "s1", back: "/setup", expires_at: 2000 });
    expect(takeSignIn(1000)).toEqual({ state: "s1", back: "/setup", expires_at: 2000 });
    expect(takeSignIn(1000)).toBeNull();
    saveSignIn({ state: "s2", back: "/setup", expires_at: 2000 });
    expect(takeSignIn(2000)).toBeNull();
  });

  it("goes back only to a page of this UpNext", () => {
    saveSignIn({ state: "s", back: "//evil.example/x", expires_at: 2000 });
    expect(takeSignIn(1000)?.back).toBe("/settings");
  });
});
