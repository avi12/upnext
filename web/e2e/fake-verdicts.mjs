// A stand-in for the backlog verdicts service (services/verdicts),
// served by fake-llm.mjs under /verdicts: it keeps verdicts in memory, one
// per key and game, so the end-to-end test sends and takes one down without
// a real service, and serves the mock library's snapshot
// (tests/fixtures/verdicts/mock/) as the daily one. GET /verdicts/_seen says
// what it holds and how often the snapshot was asked for, for the tests.
import { readFileSync } from 'node:fs'

const SNAPSHOT = new URL('../../tests/fixtures/verdicts/mock/', import.meta.url)
const verdicts = new Map()
let next = 0
let snapshotAsked = 0

function id() {
  next += 1
  return `01JA${String(next).padStart(22, '0')}`
}

function send(res, status, body) {
  if (body === undefined) res.writeHead(status).end()
  else res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
}

/** Answers a request under /verdicts; false for any other path. */
export function verdictsService(req, res, raw) {
  const path = (req.url ?? '').split('?')[0]
  if (!path.startsWith('/verdicts/')) return false
  const rest = path.slice('/verdicts'.length)
  if (req.method === 'GET' && rest === '/_seen') {
    send(res, 200, { verdicts: [...verdicts.values()], snapshot_asked: snapshotAsked })
    return true
  }
  // The published files need no key: installs download them whole.
  if (req.method === 'GET' && /^\/verdicts(-summary-\d{4}-\d{2}-\d{2}\.jsonl\.gz|\.json)$/.test(rest)) {
    if (rest === '/verdicts.json') snapshotAsked += 1
    let body
    try {
      body = readFileSync(new URL(rest.slice(1), SNAPSHOT))
    } catch {
      send(res, 404, { error: 'not found' })
      return true
    }
    res.writeHead(200, { 'content-type': rest.endsWith('.json') ? 'application/json' : 'application/gzip', etag: '"mock"' }).end(body)
    return true
  }
  const key = (req.headers.authorization ?? '').replace(/^Bearer /, '')
  if (!key.startsWith('upnext_v_')) {
    send(res, 401, { error: 'a key is needed' })
    return true
  }
  const mine = () => [...verdicts.values()].filter((v) => v.key === key)
  if (req.method === 'POST' && rest === '/v1/verdicts') {
    const body = JSON.parse(raw)
    const game = body.steam_appid ? `steam:${body.steam_appid}` : `gog:${body.gog_id}`
    const old = mine().find((v) => v.game === game)
    const v = { ...body, key, game, id: old?.id ?? id(), state: 'live' }
    verdicts.set(v.id, v)
    send(res, old ? 200 : 201, { id: v.id, state: 'live', replaced: !!old })
  } else if (req.method === 'DELETE' && rest.startsWith('/v1/verdicts/')) {
    const v = verdicts.get(rest.slice('/v1/verdicts/'.length))
    if (v && v.key === key) {
      verdicts.delete(v.id)
      send(res, 204)
    } else send(res, 404, { error: 'no such verdict' })
  } else if (req.method === 'DELETE' && rest === '/v1/verdicts') {
    const all = mine()
    for (const v of all) verdicts.delete(v.id)
    send(res, 200, { withdrawn: all.length })
  } else if (req.method === 'GET' && rest === '/v1/verdicts/mine') {
    const day = new Date().toISOString().slice(0, 10)
    const shown = mine().map((v) => {
      const out = { ...v, created_day: day, updated_day: day }
      delete out.key
      delete out.game
      return out
    })
    send(res, 200, { verdicts: shown })
  } else if (req.method === 'POST' && rest === '/v1/reports') {
    send(res, 202, {})
  } else {
    send(res, 404, { error: 'not found' })
  }
  return true
}
