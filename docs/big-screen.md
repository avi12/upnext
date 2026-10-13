# Big screen

Big screen is the same UpNext with large type and one focus you move with a gamepad, the arrow keys or a TV remote, for a Steam Deck, a Windows handheld or a computer on the TV. Turn it on in Settings › Big screen ("Switch to big screen"), by adding `?big=1` to the address, or when UpNext offers it after you first press a gamepad button; each browser remembers it.

It has Curate (your routines and the request as chips, and an on-screen keyboard for your own words), the game page with Play, the Library's covers with a few filters, Lists, Help me decide and a short Settings. Pages made for a mouse offer to leave it, and so do its Settings and the button help (View, or `?`).

| Pad | Keys | Does |
| --- | --- | --- |
| D-pad or left stick | Arrows | Move |
| A | Enter | Choose |
| B | Esc or Backspace | Back, closing a menu first |
| X | M | The card's or game's menu |
| Y | / | Change the request |
| LB, RB | [ ] | The previous or next page |
| LT, RT | Page Up, Page Down | Page through a long list |
| Menu | S | Settings |
| View | ? | The buttons |

The hints name the buttons of the pad in hand: Xbox letters, PlayStation shapes, or the Deck's. Steam's Gaming mode presents an Xbox pad, so a Deck usually shows Xbox letters; Settings in big screen can pick. A browser only sees a gamepad after one of its buttons is pressed while the page has focus, so the first press may only wake it.

A request made in big screen counts as on a controller: games Steam lists with controller support come first and ones made for a mouse fall back, unless the routine you choose says what you play on.

## The app window in big screen

`upnext app --big-screen` opens the app full screen in big screen. On Windows that's the Start menu's "UpNext (big screen)"; on Linux, the launcher entry's "Big screen" action. F11 leaves full screen and goes back, and Close UpNext, in the button help or big screen's Settings, closes it, since a full-screen window has no title bar. A window opened this way ends when it closes, with no icon left by the clock, so a launcher that started it sees it end.

An app window opened the usual way goes full screen when it switches to big screen, and leaving big screen gives back its size. Steam's Gaming mode on a Deck and Big Picture open it in big screen by themselves, and so does Steam on a Deck in Desktop mode. [Big screen with Steam](steam.md) has Add to Steam and what Play does there.

On Linux the app window reads pads through libmanette, which doesn't know an Xbox controller connected through Microsoft's wireless adapter (the `xone` driver) and gets its triggers and D-pad wrong. The window adds that controller's layout to libmanette's own file, `~/.config/libmanette/gamecontrollerdb`, leaving any line already there as it was. To take it out, delete the line that starts `# Added by UpNext` and the line under it.

## A first run with a controller

When setup opens in big screen, or a gamepad button is pressed during a first run's setup, it runs the steps that need no typing: it reads the games Steam, Heroic and Galaxy keep on the computer, then hands the rest (an AI's key, a store's sign-in) to your phone or computer with a QR code that opens setup there, paired. It ends with [Add to Steam](steam.md) and a short tour of the buttons, which the button help offers again later.

## Your phone's home screen

Once a phone is paired, the browser's menu can add UpNext to the home screen: Chrome's "Add to home screen" or "Install app", Safari's Share › "Add to Home Screen". Over HTTPS ([Other devices](other-devices.md#https-and-proxies)) Chrome opens it in its own window; over plain HTTP it is a shortcut to a browser tab. On an iPhone, the home-screen app keeps its own cookies apart from Safari's, so pair from inside it: open it, then scan or type a new code.
