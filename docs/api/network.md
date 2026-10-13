# Other devices and computers on the network

[API index](../API.md) · [Allowing a device](allowing.md)

## Listening for other devices

`allow_other_devices` (`false`) makes the server listen beyond loopback, and `tls_cert` and `tls_key` (paths, or null; set only on this computer) add HTTPS on the same port. `PUT /api/settings` applies a change to any of them before it answers: the old socket closes, the new one opens on the same port, and open connections carry on. `GET /api/settings` gives what's in effect as `listen`:

```json
"listen": {"host": "127.0.0.1", "port": 8787, "other_devices": false, "https": false, "error": null,
           "forced": false, "container": false, "this_computer": true}
```

`other_devices` when it listens beyond loopback, `https` when a certificate is loaded, and `error` when the last change didn't take (a port in use, a certificate that doesn't load). `forced` is true when `UPNEXT_HOST` is beyond loopback, as in a container, which then wins over the switch. `container` means no browser is this computer, so Settings shows `docker exec upnext upnext pair` and `… token create` instead. `this_computer` says the request came from the computer UpNext runs on, where Settings offers tokens and pairing codes.

## The announcement

While other devices may use UpNext (it listens beyond loopback, through "Allow other devices" or in a container) and the `announce` setting is on (the default), the server announces itself over mDNS and DNS-SD as a `_upnext._tcp.local.` service on its port. Its TXT values are `v=1`, `id` (a random install id, kept in the database's `meta`), `name` (the `network_name` setting, else the host name), `tls` (`0` or `1`) and `ver` (UpNext's version). The service record's host is UpNext's own, `upnext-<id>.local.`, never the computer's name, which Avahi or Bonjour already answers for. Nothing about the library or its people is announced. A container on Docker's default bridge network announces only inside it; with `network_mode: host` it reaches the network.

Settings: `announce` (`true` by default) and `network_name` (1 to 63 bytes; unset, the host name). A change to either applies at once, like `allow_other_devices`. `GET /api/settings` also gives `network_name_shown`, the name in use.

"Allow other devices" listens on `0.0.0.0`, IPv4 only, so a device that reaches this computer only over IPv6 can't connect. Set `host` (`UPNEXT_HOST`) to `::` for both, where the system's sockets take IPv4 on it too.

## Using another computer's library

The app window on this computer can use another computer's UpNext instead of its own (`upnext app --connect`, or "Use the library on another computer" in the setup wizard). Its connect page uses these, all on this computer only, since they make the server call other computers and keep the tokens they give.

### GET /api/network/computers

```json
{"this": {"id": "…", "name": "attic"},
 "found": [{"id": "4f2a9c01d3e85b67", "name": "den", "version": "0.1.0", "address": "http://192.168.0.42:8787"}],
 "saved": [{"id": "…", "name": "den", "address": "http://192.168.0.42:8787", "open": true}]}
```

`found` is the computers announcing UpNext now, without this one (it listens for about two seconds). `saved` is the ones this computer was allowed on before, kept in `computers.json` in the config folder, readable by this user alone. `open` marks the one the window opens on.

### DELETE /api/network/computers/{id}

`204`: forget one and its token here. It stays in that computer's device list until revoked there.

### POST /api/network/connect

`{"address": "den.lan", "id": "4f2a9c01d3e85b67"}`. The address is a found one or one typed (`den.lan`, `192.168.0.42:8787`, `https://den.tailnet.ts.net`); without a scheme it's `http://`, and without a port 8787. `id`, optional, is the install id it was found with. After checking the address answers as UpNext, it asks that computer (`POST /api/pair/request` there, wanting a token) and answers `201`:

```json
{"status": "waiting", "match_code": "4821", "computer": "den", "address": "http://den.lan:8787", "id": null}
```

`400` for something that isn't an address; `409` when `id` is given and another UpNext answers there; `502` when nothing answers there as UpNext or it refused (its reason is given). Any earlier request from here ends.

### GET /api/network/connect

The same shape, after asking the other computer once: `waiting`, `allowed` (its token is saved, and `id` names it), `denied` or `expired`. `404` when no request is waiting; `502` when the other computer doesn't answer.

Once allowed, the page posts `connect:<id>` to the window, which opens on that computer and keeps opening on it until the page asks for this computer's library again (`connect:here`, from Settings › Devices and API on the other computer's page). Each time, before the token goes there, the window asks the address's `GET /api/health`, with no token, whether its `id` is still that computer's. When another UpNext answers it opens `/connect?changed=<id>` here instead, and when nothing does, `/connect?unreachable=<id>`. Trust is on first use: the computer that allowed this one is taken to be the one that answers with its id, and over plain HTTP the token crosses the home network as any paired device's cookie does.

### DELETE /api/network/connect

`204`: stop waiting.
