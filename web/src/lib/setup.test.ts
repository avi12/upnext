import { describe, expect, it } from "vitest";
import type { SetupInfo } from "../types";
import {
  backgroundAsk,
  backgroundHint,
  claudeInstall,
  flatpakClaude,
  flatpakFolders,
  mountLines,
  noWindowText,
  runsWhileText,
  whereText,
} from "./setup";

type Env = SetupInfo["environment"];
const env = (over: Partial<Env> = {}): Env => ({
  os: "linux",
  mode: "serve",
  container: false,
  flatpak: null,
  display: true,
  gog_window: true,
  client_local: true,
  ...over,
});

describe("where UpNext runs", () => {
  it("says the app window apart from a browser, whichever server it shows", () => {
    expect(whereText(env({ mode: "app" }), true)).toBe(
      "You're running the UpNext app on Linux.",
    );
    expect(whereText(env({ mode: "service" }), true)).toBe(
      "You're in the UpNext app, showing the UpNext service on this Linux computer.",
    );
    expect(whereText(env({ mode: "serve", os: "windows" }), true)).toContain(
      "started from a terminal on this Windows computer",
    );
    expect(whereText(env({ mode: "app" }), false)).toBe(
      "UpNext runs inside the UpNext app on this Linux computer, and you're in a browser on it.",
    );
    expect(
      whereText(env({ mode: "service", client_local: false }), false),
    ).toBe(
      "UpNext runs as a service on a Linux computer, and you're on another device.",
    );
    expect(
      whereText(env({ container: true, mode: "service" }), true),
    ).toContain("Docker container");
    const flatpak = "io.github.zytocc.UpNext";
    expect(whereText(env({ mode: "app", flatpak }), true)).toBe(
      "You're running the UpNext app on Linux, as a Flatpak.",
    );
    expect(whereText(env({ mode: "app", flatpak }), false)).toBe(
      "UpNext runs as a Flatpak on this Linux computer, and you're in a browser on it.",
    );
  });

  it("says what a Flatpak reads, and how to give it more", () => {
    const id = "io.github.zytocc.UpNext";
    const { text, commands } = flatpakFolders(id);
    expect(text).toContain(`~/.var/app/${id}`);
    expect(commands[0]).toBe(
      `flatpak override --user --filesystem=/path/to/Steam:ro --env=UPNEXT_STEAM_DIR=/path/to/Steam ${id}`,
    );
    expect(commands[1]).toContain("--env=UPNEXT_HEROIC_DIR=/path/to/heroic");
    expect(flatpakClaude(id)).toEqual({
      install: `flatpak run --command=upnext ${id} install-claude`,
      token: `flatpak override --user --env=CLAUDE_CODE_OAUTH_TOKEN=<the token> ${id}`,
    });
    expect(backgroundHint(env({ mode: "app", flatpak: id }))).toBeNull();
  });

  it("says how long it keeps running, and how to keep it running on Linux", () => {
    expect(runsWhileText(env({ mode: "app" }))).toContain(
      "while the app's window is open",
    );
    expect(runsWhileText(env({ mode: "service" }))).toContain("keeps running");
    expect(runsWhileText(env({ mode: "serve" }))).toContain("terminal");
    expect(
      runsWhileText(env({ mode: "serve", client_local: false })),
    ).toBeNull();
    expect(runsWhileText(env({ container: true }))).toBeNull();
    expect(backgroundHint(env({ mode: "app" }))).toContain("Start it at login");
    expect(backgroundHint(env({ mode: "service" }))).toBeNull();
    expect(backgroundHint(env({ mode: "app", os: "windows" }))).toBeNull();
  });
});

describe("GOG's window", () => {
  it("says nothing where it opens, or for another device or a container", () => {
    expect(noWindowText(env())).toBeNull();
    expect(
      noWindowText(
        env({ gog_window: false, display: false, client_local: false }),
      ),
    ).toBeNull();
    expect(
      noWindowText(env({ gog_window: false, display: false, container: true })),
    ).toBeNull();
  });

  it("says why not, and how to fix a Linux service", () => {
    expect(noWindowText(env({ gog_window: false }))?.why).toContain(
      "built without its windows",
    );
    const service = noWindowText(
      env({ gog_window: false, display: false, mode: "service" }),
    );
    expect(service?.commands).toEqual([
      "systemctl --user import-environment DISPLAY XAUTHORITY",
      "systemctl --user restart upnext",
    ]);
    expect(
      noWindowText(
        env({
          os: "windows",
          gog_window: false,
          display: false,
          mode: "service",
        }),
      )?.why,
    ).toContain("Windows sign-in");
    expect(
      noWindowText(env({ gog_window: false, display: false }))?.commands,
    ).toBeUndefined();
  });
});

it("installs Claude Code the way each system does", () => {
  expect(claudeInstall("windows")).toEqual({
    command: "irm https://claude.ai/install.ps1 | iex",
    shell: "PowerShell",
  });
  expect(claudeInstall("linux").command).toBe(
    "curl -fsSL https://claude.ai/install.sh | bash",
  );
});

describe("mountLines", () => {
  it("mounts the chosen Linux folders read-only, with their variables", () => {
    const { compose, run, mounted } = mountLines({ host: "linux", steam: "flatpak", heroic: "native", tz: "America/New_York" });
    expect(mounted).toBe(2);
    expect(compose).toBe(
      [
        "    volumes:",
        "      - upnext-data:/data",
        '      - "~/.var/app/com.valvesoftware.Steam/.local/share/Steam:/host/steam:ro"',
        '      - "~/.config/heroic:/host/heroic:ro"',
        "    environment:",
        "      TZ: America/New_York",
        "      UPNEXT_STEAM_DIR: /host/steam",
        "      UPNEXT_HEROIC_DIR: /host/heroic",
      ].join("\n"),
    );
    expect(run).toContain('-v "$HOME/.var/app/com.valvesoftware.Steam/.local/share/Steam:/host/steam:ro" -e UPNEXT_STEAM_DIR=/host/steam \\\n');
    expect(run).toContain("-e TZ=America/New_York");
    expect(run.split("\n").at(-1)).toBe("  ghcr.io/zytocc/upnext:latest");
    // As compose.yaml runs it: read-only, no capabilities, no new privileges.
    expect(run).toContain("--read-only --tmpfs /tmp --security-opt no-new-privileges --cap-drop ALL");
  });

  it("leaves out a library set to none", () => {
    const { compose, run, mounted } = mountLines({ host: "linux", steam: "snap", heroic: "none", tz: "UTC" });
    expect(mounted).toBe(1);
    expect(compose).toContain("~/snap/steam/common/.local/share/Steam:/host/steam:ro");
    expect(compose + run).not.toContain("heroic");
  });

  it("writes Windows paths for compose and PowerShell", () => {
    const { compose, run } = mountLines({ host: "windows", steam: "native", heroic: "native", tz: "UTC" });
    expect(compose).toContain('- "C:/Program Files (x86)/Steam:/host/steam:ro"');
    expect(compose).toContain('- "${APPDATA}/heroic:/host/heroic:ro"');
    expect(run).toContain('-v "C:\\Program Files (x86)\\Steam:/host/steam:ro"');
    expect(run).toContain('-v "$env:APPDATA\\heroic:/host/heroic:ro"');
    expect(run.split("\n")[0]).toMatch(/ `$/);
  });

  it("quotes macOS's folders, which have spaces", () => {
    const { run } = mountLines({ host: "macos", steam: "native", heroic: "native", tz: "UTC" });
    expect(run).toContain('-v "$HOME/Library/Application Support/Steam:/host/steam:ro"');
  });
});

describe("what moving between steps starts in the background", () => {
  it("syncs once the stores are behind, and holds details until the keys are", () => {
    expect(backgroundAsk("stores", "extras")).toEqual({ sync: "if_changed", details: "wait" });
    // Epic's sign-in is on the second store step: the sync waits for it.
    expect(backgroundAsk("stores", "more_stores")).toBeNull();
    expect(backgroundAsk("more_stores", "extras")).toEqual({ sync: "if_changed", details: "wait" });
    expect(backgroundAsk("extras", "ai")).toEqual({ details: "after_sync" });
    expect(backgroundAsk("steam", "ai_jobs")).toEqual({ sync: "if_changed", details: "after_sync" });
    expect(backgroundAsk("where", "sync")).toEqual({ sync: "if_changed", details: "after_sync" });
  });

  it("starts nothing going back or moving on later", () => {
    expect(backgroundAsk("extras", "stores")).toBeNull();
    expect(backgroundAsk("welcome", "stores")).toBeNull();
    expect(backgroundAsk("ai", "sync")).toBeNull();
    expect(backgroundAsk("stores", "stores")).toBeNull();
    // An imported database jumps from the welcome step with a library of its own.
    expect(backgroundAsk("welcome", "ai")).toBeNull();
    expect(backgroundAsk("welcome", "done")).toBeNull();
  });
});
