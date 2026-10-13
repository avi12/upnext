# Answers for Steamworks' content survey

Drafts for the questions Steamworks asks about a product's content. The survey's exact wording changes; answer what it asks with these facts, and re-read each against UpNext as it is when the page is submitted.

## Mature content

None. UpNext shows the names, cover art and store descriptions of games the person already owns, and the summaries its AI writes of them. It has no violence, nudity, gambling or chat of its own. Cover art and store text come from the stores as they are; a game in the person's library with a mature rating is shown as the store shows it.

## AI-generated content

Steam asks whether a product contains pre-generated AI content (made during development and shipped) and live-generated content (made while the product runs).

- **Pre-generated: yes.** UpNext is developed with AI coding assistants, mostly Claude Code, and every change goes through the same tests and review as any other (the README's "Contributing"). The shared game data UpNext can apply (`upnext seed apply`: "People say", vibes and review tags for the most-owned games on Steam) is written by an AI model from public player reviews before a release, then published with it under CC0. The app's own text, icons and images are its contributors' work.
- **Live-generated: yes.** While UpNext runs, an AI writes the picks and their reasons, short summaries of what players say about a game, series guides and second-chance notes. The AI is the one the person sets up and pays for themselves (their own Claude or ChatGPT plan through its command-line tool, an API key, or a model on their own computer); UpNext doesn't include or sell one, and works without one.

How the live-generated content is bounded, for the survey's description of guardrails:

- It describes games in the person's own library (and, with the wishlist on, games that follow on from series they played), from the stores' and game databases' data and public player reviews. Every pick must be one of the games UpNext offered the AI from the library: an answer that names anything else is refused and asked again, and the names shown come from UpNext's own database, not the AI.
- It is text only: UpNext generates no images, audio or voices.
- Nothing generated is shared with other players. It stays in the database on the person's computer.
- The person sees it only on request (a pick they asked for, a summary job they turned on), and can rate a pick or turn a lineup down.

## Other questions

- **Accounts:** UpNext has no account of its own. Its Steam sign-in is Steam's own OpenID page in the person's browser, and it never asks for or stores a Steam password; a Steam Web API key is the person's own and optional.
- **Data collection:** none by UpNext. It has no analytics or telemetry; its one server, the backlog verdicts service, receives only the verdicts a player chooses to publish, under CC0, with an anonymous key. It talks to the stores and game databases it reads and to the person's chosen AI ([Privacy](../../../docs/privacy.md)).
- **Advertising, purchases, cryptocurrency or NFTs:** none.
- **Network:** needs the internet to read online libraries and store details, and for a hosted AI; works on the home network for a phone or another computer the person allows.
