# Commands

`upnext --help` lists every command, and `upnext <command> --help` its options. The common ones:

```sh
upnext serve [--port 8787]       # the web UI and API; runs the scheduled jobs while it's up
upnext app [--port 8787]         # the UI in a window
upnext app --big-screen          # full screen, for a controller
upnext app --connect             # choose another computer's UpNext for the window
upnext doctor                    # every path in use and where it came from; Steam, GOG, the AI CLI

upnext sync [--skip heroic] [--only steam] [--steam-local] [--galaxy-db <file>]
upnext enrich [--with hltb,digests] [--skip rawg] [--only igdb] [--limit N] [--refresh]   # --help lists the sources
upnext jobs [history [--failed] | log <#> | schedule <job> <when>]

upnext pick "2 hours, chill, story-heavy" [-n 3] [--source gog] [--json]
upnext pick "new to me"          # a way into a genre you haven't played; "a hidden gem" too
upnext pick --without-ai "..."   # from UpNext's own ranking
upnext prompt "..."              # print the exact prompt a pick would send
upnext list [--status untouched,sampled] [--source steam] [--sort review] [-q hike]
upnext show <ref>                # ref = Steam appid | source:id | #internal-id
upnext mark done|shelved|abandoned|hidden|still_playing|burnt_out <ref> [--note "..."]
upnext unmark <ref>
upnext merge "#12" "#345"        # one game listed as several titles becomes one
upnext split gog:1207659068      # a store copy becomes its own title
upnext stats
upnext taste [--update]          # your taste profile and routines

upnext backup [--keep 4] [--note "..."]
upnext export <file> [--from <backup>] [--force]   # a copy without keys and sign-ins
upnext seed apply [<file or url>]   # shared summaries for popular games, no AI needed; also status, remove
upnext verdicts list|send|withdraw|refresh|sync|apply   # backlog verdicts
upnext config list|set|unset

upnext gog-login | gog-logout
upnext epic-login | epic-logout
upnext token create --name phone --scope read      # also list, revoke; --player Sam binds it to one player
upnext player list|add|rename|remove|pin           # `pin Sam --clear` clears a forgotten PIN
upnext pair                      # a pairing code from the running server
upnext pair requests             # devices asking to be allowed, with their numbers
upnext pair approve 4821         # allow the one showing 4821; `deny` refuses it

upnext install-desktop [--remove]   # the launcher entry on Linux
upnext migrate-data --from ./data
upnext mock-library [--one-player]  # a made-up library and profile, into an empty database
upnext healthcheck
upnext install-claude            # Claude Code from Anthropic, into UpNext's home folder
```

`--player <name or id>` (or `UPNEXT_PLAYER`) before any command runs it for one player ([Players](players.md)).
