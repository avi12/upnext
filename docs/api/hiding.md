# Hiding games

[API index](../API.md) · [Players](players.md) · [Accounts](accounts.md)

- `PUT /api/games/{id}/hide` `{"scope": "me" | "others" | "everyone"}` answers `{"scope", "still_seen_by": ["Sam", "Kit"], "through_shared": ["Kit"]}`. `me` takes it out of your own library, search, picks and statistics; `others` keeps your copies out of every other player's, unless they have it through their own account or another shared one; `everyone` does both. `others` and `everyone` need a copy of your own (`400`). It isn't Not for me, so it teaches the taste profile nothing. `still_seen_by` names who still sees it, and `through_shared` those who see it only through another player's shared account; both are empty for `me`.
- `DELETE /api/games/{id}/hide` answers `204`.
- A `GameView` says how the request's player hid it, `hidden_scope` (`me`, `others`, `everyone` or null), and `own_copy`, true when one of its copies is in a store account of theirs. Another player's hiding is never shown.
- `GET /api/hidden` lists Hidden by me: `[{"title_id", "name", "scope", "at", "own_copy"}]`, newest first. For a player with a PIN, only within ten minutes of `POST /api/hidden/unlock` `{"pin"}` in this browser or with this token (`403` otherwise), and their hidden games' pages open in that time too. `DELETE /api/hidden/unlock` locks it again before the ten minutes are up (`204`).
- A game hidden from a player is never named to them: in jobs, job logs and `GET /api/logs` it reads "a hidden game".
