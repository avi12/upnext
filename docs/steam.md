# Big screen with Steam

UpNext's [big screen](big-screen.md) is made for Steam's own: a Deck's Gaming mode, Big Picture, or a handheld's Steam.

## Playing from Steam's big screen

When Big Picture or a Deck's Gaming mode started UpNext, pressing Play closes UpNext a couple of seconds after the game's launcher has the game. Steam counts UpNext as the game that is running, so without that, Big Picture would keep waiting on UpNext after the game ends. Start UpNext from Steam again for the next pick. If the launcher couldn't be opened, UpNext stays, and it stays for a GOG game through Galaxy too, whose link only shows the game in Galaxy. Started any other way, including from Steam's desktop window or a Deck's Desktop mode, UpNext stays open when a game starts.

## Add to Steam

Settings › Big screen, on the computer UpNext runs on, has **Add to Steam**. It puts UpNext in the Steam library of every account on that computer, under Non-Steam, with its own artwork, opening in big screen, so Big Picture, a Deck's Gaming mode or a handheld's Steam can start it. Close Steam first: Steam rewrites its list of shortcuts as it exits.

UpNext copies Steam's file aside (`shortcuts.vdf.upnext-backup`) before changing it, leaves your other shortcuts as they were, and adds itself once however often you press it. It also gives the shortcut Steam's Gamepad controller layout, since Steam otherwise gives a non-Steam shortcut the desktop layout, where the pad moves the mouse. If it still does (Steam Cloud can put the old setting back), pick it by hand on UpNext's page in Steam: the controller icon, the layout, Templates, Gamepad.

The same section shows the program and arguments to give a handheld's own launcher (the Xbox app, Armoury Crate SE, Legion Space), with a copy button, since none of those lets a program add itself. On Windows, picking "UpNext (big screen)" from the Start menu's apps does the same. In the Flatpak, Add to Steam says how to add UpNext in Steam by hand instead ([Flatpak and Steam Deck](install/flatpak.md#on-a-steam-deck)).

