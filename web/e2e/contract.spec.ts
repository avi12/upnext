import { expect, test } from '@playwright/test'
import { fileURLToPath } from 'node:url'
import ts from 'typescript'

// The hand-written response types in src/types.ts against what the server
// answers on the mock library, key by key, three levels down (code health
// R-WEB-5). Many of the server's answers are untyped in docs/openapi.json,
// so neither `npm run gen:api` nor `tsc` would notice a field renamed on one
// side only; this does. A key the type requires and the answer leaves out
// fails the test, and so does a key the answer has and the type doesn't,
// unless it is listed below as one the UI doesn't read. Values aren't
// compared, only which keys there are, and a record (`Record<string, T>`)
// is left alone, since its keys are data.

const file = fileURLToPath(new URL('../src/types.ts', import.meta.url))
const program = ts.createProgram([file], {
  strict: true,
  skipLibCheck: true,
  noEmit: true,
  module: ts.ModuleKind.ESNext,
  target: ts.ScriptTarget.ES2022,
  moduleResolution: ts.ModuleResolutionKind.Bundler,
  allowImportingTsExtensions: true,
})
const checker = program.getTypeChecker()
const types = new Map<string, ts.Type>()
ts.forEachChild(program.getSourceFile(file)!, (node) => {
  if (ts.isInterfaceDeclaration(node) || ts.isTypeAliasDeclaration(node)) {
    types.set(node.name.text, checker.getTypeAtLocation(node.name))
  }
})

// Keys the server answers that the UI doesn't read, so types.ts leaves them
// out on purpose. A key added to an answer fails the test until it is typed
// or listed here, and an entry no answer has any more fails it too.
const unread = new Set([
  // The app window reads its own size and zoom from /settings; the UI reads
  // the last sync from /profile, and the rest of these nowhere.
  'Config.epic_connected',
  'Config.gog_connected',
  'Config.steam_api_configured',
  'Config.steam_id_source',
  'Config.steam_local_dir',
  'Settings.last_sync',
  'Settings.player_keys',
  'Settings.steam_local_dir',
  'Settings.window_maximized',
  'Settings.window_size',
  'Settings.window_zoom',
  // The verdicts contract is docs/verdicts-contract.json whole; the form
  // reads its questions, choices and limits for the text it shows.
  'VerdictContract.upnext_verdicts_contract',
  'VerdictContract.format',
  'VerdictContract.key_prefix',
  'VerdictContract.licence',
  'VerdictContract.licence_url',
  'VerdictContract.consent',
  'VerdictContract.vocab',
  'VerdictContract.limits.client_chars',
  'VerdictContract.limits.percent_at',
  'VerdictContract.limits.show_at',
  'VerdictContract.limits.texts_per_game',
  'VerdictContract.fields[].choices[].line',
  'VerdictContract.fields[].choices[].review_tag',
  'VerdictContract.fields[].choices[].aspects',
])
const unreadSeen = new Set<string>()

interface Key {
  optional: boolean
  type: ts.Type
}

/** A type's keys, from every object in a union: a key some of its objects
 *  leave out is optional. */
function keys(type: ts.Type): Map<string, Key> {
  const parts = (type.isUnion() ? type.types : [type]).filter((p) => p.flags & (ts.TypeFlags.Object | ts.TypeFlags.Intersection))
  const out = new Map<string, Key>()
  for (const part of parts) {
    for (const p of checker.getPropertiesOfType(part)) {
      const optional =
        !!(p.flags & ts.SymbolFlags.Optional) ||
        out.get(p.name)?.optional === true ||
        parts.some((other) => checker.getPropertyOfType(other, p.name) === undefined)
      out.set(p.name, { optional, type: checker.getTypeOfSymbol(p) })
    }
  }
  return out
}

/** An array's element type, or the type itself. */
function element(type: ts.Type): ts.Type {
  const t = checker.getNonNullableType(type)
  return checker.isArrayType(t) ? checker.getTypeArguments(t as ts.TypeReference)[0] : t
}

/** Every difference between `type` and `json`, `depth` levels down. */
function differences(label: string, type: ts.Type, json: unknown, depth: number): string[] {
  if (json === null || typeof json !== 'object') return []
  if (Array.isArray(json)) {
    return json.slice(0, 20).flatMap((item) => differences(`${label}[]`, element(type), item, depth))
  }
  const t = checker.getNonNullableType(type)
  if (checker.getIndexInfosOfType(t).length > 0) return []
  const expected = keys(t)
  if (expected.size === 0) return []
  const found = json as Record<string, unknown>
  const out: string[] = []
  for (const [k, v] of expected) {
    if (!(k in found) && !v.optional) out.push(`${label}.${k}: in types.ts, not in the answer`)
  }
  for (const k of Object.keys(found)) {
    if (expected.has(k)) continue
    if (unread.has(`${label}.${k}`)) unreadSeen.add(`${label}.${k}`)
    else out.push(`${label}.${k}: in the answer, not in types.ts`)
  }
  if (depth > 0) {
    for (const [k, v] of expected) {
      if (found[k] && typeof found[k] === 'object') out.push(...differences(`${label}.${k}`, element(v.type), found[k], depth - 1))
    }
  }
  return out
}

// Each endpoint the UI reads, with the type it reads it as.
const endpoints: [string, string][] = [
  ['/api/v1/settings', 'Settings'],
  ['/api/v1/config', 'Config'],
  ['/api/v1/games?limit=5', 'GamesResponse'],
  ['/api/v1/players', 'PlayersResponse'],
  ['/api/v1/accounts', 'AccountsResponse'],
  ['/api/v1/taste', 'TasteResponse'],
  ['/api/v1/taste/sources', 'TasteSources'],
  ['/api/v1/jobs', 'JobsResponse'],
  ['/api/v1/jobs/scheduled', 'ScheduledResponse'],
  ['/api/v1/wishlist', 'WishlistResponse'],
  ['/api/v1/second-chances', 'SecondChancesResponse'],
  ['/api/v1/gems', 'GemsResponse'],
  ['/api/v1/tokens', 'TokensResponse'],
  ['/api/v1/profile', 'Profile'],
  ['/api/v1/stats', 'Stats'],
  ['/api/v1/setup', 'SetupInfo'],
  ['/api/v1/routines', 'Routines'],
  ['/api/v1/llm/models', 'LlmModels'],
  ['/api/v1/review-tags', 'ReviewVocabulary'],
  ['/api/v1/verdicts/vocab', 'VerdictContract'],
  ['/api/v1/sync/sources', 'SyncLibrary'],
  ['/api/v1/enrich/sources', 'EnrichSource'],
  ['/api/v1/picks/history?limit=2', 'PickBatch'],
  ['/api/v1/collections', 'Collection'],
  ['/api/v1/ai-accounts', 'AiAccount'],
]

test('the hand-written response types match what the server answers', async ({ request }) => {
  // A lineup drawn by chance, with no AI call, so the history has a batch to
  // compare however many lineups the specs before this one made.
  const rolled = await request.post('/api/v1/picks', { data: { mode: 'random', count: 1 } })
  expect(rolled.status()).toBe(200)
  const found = differences('PickResult', types.get('PickResult')!, await rolled.json(), 3).map((d) => `POST /api/v1/picks: ${d}`)
  for (const [path, name] of endpoints) {
    const type = types.get(name)
    expect(type, `${name} is in src/types.ts`).toBeDefined()
    const res = await request.get(path)
    expect(res.status(), path).toBe(200)
    found.push(...differences(name, type!, await res.json(), 3).map((d) => `${path}: ${d}`))
  }
  for (const k of unread) if (!unreadSeen.has(k)) found.push(`${k}: listed as unread, but no answer has it`)
  expect([...new Set(found)]).toEqual([])
})
