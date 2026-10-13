# Backlog verdicts

A backlog verdict is a short public answer about how a game fits a backlog and limited time: how far you got, whether it was worth it, how long until it clicked, the sessions it needs, whether it's fine to stop after the credits, and what made you put it down. Every install that turns them on reads everyone's answers on game pages, pick cards and the Library's filters.

## Writing one

A game's page has **Write a backlog verdict**, and after you mark a game finished, shelved, abandoned, burnt out or not for me it offers one in a line (Settings › Library data turns the offer off). UpNext fills in what it already knows from your mark, playtime, play diary and the reasons you gave for stopping; you change or add the rest, and a short text is optional.

Before your first verdict leaves the computer, a consent screen says plainly that it will be public under [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/), so anyone may copy and reuse it. Nothing is sent until you press Send.

## What is sent

The answers, the text, the game's Steam or GOG id, the versions of the questions and the consent you accepted, the UpNext version, and your player's random key, made on your first send. Never your name, a store account, your library, your exact playtime (hours go as a range) or anything else from your settings. The service keeps only a hash of the key, and the published files don't say which verdicts came from the same key. [A verdict](verdicts/verdict.md) has the full list.

## Taking it down

**Take down** on the game page withdraws a verdict, and **Take down all my verdicts** in Settings › Library data withdraws them all and forgets your key. A withdrawn verdict is left out of the next daily update and gone from the service within a week, but copies people already downloaded can't be recalled.

## Reading other players' verdicts

**Backlog verdicts from other players**, in Settings › Library data and in the setup wizard beside UpNext's game data, downloads everyone's verdicts once a day, whole, and sends nothing about you. The last download stays for offline use. A second switch there decides whether the AI that picks reads them too; your own marks and taste always count for more.

## The service

UpNext's own service is at `https://verdicts.upnext.games`, run by UpNext contributors; its terms are at [`/terms`](https://verdicts.upnext.games/terms) and on the consent screen. Anyone can run their own from `services/verdicts/` and set its address in Settings › Library data, on the computer UpNext runs on, or in `verdicts_url`. A computer that can't reach it can put a downloaded snapshot in place with `upnext verdicts apply`. [The contract](VERDICTS.md) describes the endpoint and the files.
