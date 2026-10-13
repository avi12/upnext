# Phones and other computers

Both the app window on another computer and the Android app get a token when they're allowed, and both trust the computer on first use. [Who can reach UpNext](devices.md) has the rules every device follows.

## The app window on another computer

The window trusts on first use. The computer that allowed it gave its install id with the token. Each time the window opens there, it first asks that address's health check, with no token, for the id, and sends the token only when the id matches; otherwise it says another UpNext answers there now. The id is public, so this keeps a token from going to another computer that took the address, not from one set up to pretend. Over plain HTTP the token crosses the home network as a paired phone's cookie does, so use HTTPS (a certificate, or `tailscale serve`) where the network isn't trusted. The token is kept in `computers.json` in the config folder, readable by this user alone, and never logged.

## The Android app

The app asks the same way, or pairs with the code, and keeps each computer's token, never the code, sealed with an AES key in Android's keystore. Its data is left out of cloud backups and device transfers. On Android 17 it finds computers through the system's picker, which needs no permission, and asks for Nearby devices only before connecting to a typed or scanned address that may be on the home network.

It uses plain HTTP only with a host whose addresses are all private (home ranges and Tailscale's 100.64.0.0/10), link-local or loopback, the same rule the server takes a token by, and connects to the address it checked. It holds a computer's public address to the same rule, and refuses anything else until given an `https://` address. It trusts certificate authorities the user installed on the phone as well as the system's. In its WebView the token is a session cookie, set when a computer is opened and cleared when it is left.

Like the app window, it trusts on first use. Before sending a saved computer its token, it asks that address's health check for the id; another UpNext answering there, or one with no id, gets nothing. A device that copies the id would pass, so the token goes only to the address it last worked at. When a computer has moved, the app names both addresses and sends the token to the new one only after the person taps to use it. Pairing stops if the computer that answers has another id than the one it was found with. Forgetting a computer asks it to revoke the token; a lost phone is revoked in Settings › Devices and API.
