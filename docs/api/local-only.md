# Only on this computer

[API index](../API.md) · [Who can use the API](access.md)

Whatever the token, these answer only on the computer UpNext runs on, so a token or a paired device can't hand out more access or reach the server's files. Anything else gets `403`. That holds behind a proxy on this computer too, where every request comes from loopback: the rule is who asks (no token, a loopback peer, a loopback `Host`), not where the connection comes from.

- `POST /api/gog/window`, which opens a window on the server's desktop;
- making a token (`POST /api/tokens`) or a pairing code (`POST /api/pairing`), and reading or ending a waiting code (`GET` and `DELETE /api/pairing`, `GET /api/pairing/outcome`);
- listing and answering devices that ask to be allowed (`GET /api/pairing/requests`, `POST /api/pairing/requests/{id}`);
- setting `galaxy_db`, `tls_cert` or `tls_key` through `PUT /api/settings`, which name files on the server, and a sync job's own `galaxy_db` other than the one set (`POST /api/jobs`);
- setting `galaxy_db`, `heroic_dir` or `epic_manifests` on a store account (`POST` and `PUT /api/connections`), for the same reason;
- `start_with_windows`, `keep_running` and `verdicts_url` through `PUT /api/settings`, which change what runs on this computer and where it sends verdicts;
- how many backups are kept (a backup job's `keep`, and `keep` in `PUT /api/jobs/scheduled/backup`), since keeping fewer removes the rest;
- downloading, restoring, importing and deleting backups, and clearing things before the setup wizard starts again (`POST /api/setup/reset`);
- Add to Steam (`GET` and `POST /api/steam/shortcut`), which reads and writes Steam's files there;
- using another computer's UpNext from this one (`/api/network/…`);
- the sign-in settings (`oidc_issuer`, `oidc_client_id`, `oidc_client_secret`, `oidc_name`, `oidc_auto_provision`, `oidc_allowed_groups`, `trusted_proxies` and `trusted_proxy_login` through `PUT /api/settings`), since they decide who gets in, and linking a player to an identity provider account (`POST` and `DELETE /api/players/{id}/oidc/link`, `POST /api/players/{id}/oidc/confirm`, `DELETE /api/players/{id}/oidc`).
