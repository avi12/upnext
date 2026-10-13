# The store page's text

Drafts for UpNext's page on Steam, in plain language, for the maintainer to paste into Steamworks (Store presence, Edit store page) and adjust. `docs/dev/RELEASE.md`, "The Steam copy", has the order of the steps around them.

| File | Where it goes |
|---|---|
| `short-description.txt` | Basic info, the short description (Steam allows 300 characters; this is 277). |
| `about.txt` | Description, "About This Software", in Steam's own markup (`[h2]`, `[list]`, `[*]`). |
| `content-survey.md` | The content survey: mature content and the AI disclosure, with how its live-generated text is bounded. |

The rest of the page:

- **Name:** UpNext. **Developer and publisher:** UpNext contributors.
- **Type and category:** software, under Player Tools, which is where Steam takes non-game products like UpNext. Genre tags, if Steamworks asks: Utilities.
- **Price:** free, with no purchases inside.
- **Supported languages:** English (interface and text; no audio).
- **Controller:** full controller support in big screen, which the "UpNext" launch option opens. Steam Deck compatibility review is open only to games, so the page should have no Deck badge; confirm that on the app's Steam Deck compatibility page in Steamworks.
- **Website:** `https://github.com/zytocc/upnext`. **Support:** UpNext is provided as is, with no support or warranty; questions and bug reports go to `https://github.com/zytocc/upnext/issues`, answered when someone can. No support email. Steam doesn't make links in the description clickable, so the addresses go in these fields rather than in `about.txt`.
- **Privacy:** [Privacy](../../../docs/privacy.md), `https://github.com/zytocc/upnext/blob/main/docs/privacy.md`, if Steamworks asks for a privacy policy.
- **Legal line:** "UpNext is free software under the MIT or Apache 2.0 licence. UpNext isn't affiliated with Valve, GOG, Epic, Amazon, Electronic Arts or Ubisoft."
- **System requirements:**
  - Windows: Windows 10 or 11, 64-bit, with Microsoft's WebView2 Runtime (Windows 11 has it; UpNext offers the download if it's missing). About 100 MB of disk.
  - SteamOS and Linux: SteamOS 3, or a 64-bit Linux with glibc 2.35 or newer (Ubuntu 22.04, Debian 12, Fedora 36 and later). About 350 MB of disk.
  - Both: an internet connection to read online libraries and store details, and for a hosted AI. An AI is optional.

## Images

Every image is made from UpNext's own screens on the made-up library from `upnext mock-library`, never a real one. The capsules carry UpNext's name, as Steam's guidelines for them ask; the screenshots are the app's screens with nothing added. Steamworks' Graphical assets page has the current sizes, so check there first.

- **Screenshots:** at least five at 1920×1080: big screen on Curate (at 1280×800, the Deck's size, centred), a lineup with its reasons, a game page, Help me decide, and the Library. The README's and Flathub's shots come from `web/e2e/readme-shot.spec.ts`; the same setup at 1920×1080 makes these.
- **Store capsules:** header 920×430, small 462×174, main 1232×706, vertical 748×896; a page background (1438×810) is optional.
- **Library:** capsule 600×900, header 920×430, hero 3840×1240 and a logo with a transparent background (up to 1280 wide or 720 tall). `contrib/steam/library/` has these four from "Add to Steam" to start from.
- **Icons:** the community icon (184×184) and the client icon Steamworks asks for, from `web/public/` and `contrib/windows/upnext.ico`.
- **Trailer:** optional.
