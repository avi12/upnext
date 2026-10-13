// Checks the licence of every npm package in the service's package-lock.json,
// as web/scripts/licenses.mjs does for the UI. No npm package ships in the
// deployed Worker: its bundle is the service's own code and the contract file
// (`npx wrangler deploy --dry-run --outdir dist` shows it), so every package
// here is a tool that runs only on a developer's machine or in CI. Prints a
// count per licence; exits 1 on anything not allowed, or a package with no
// licence. `npm run licenses`; CI runs it.
import { readFileSync } from 'node:fs'

const lock = JSON.parse(readFileSync(new URL('../package-lock.json', import.meta.url), 'utf8'))

// The same list as the UI's build tools.
const TOOLS = new Set(['MIT', 'MIT-0', 'ISC', '0BSD', 'BSD-2-Clause', 'BSD-3-Clause', 'Apache-2.0', 'CC0-1.0', 'OFL-1.1', 'MPL-2.0', 'Python-2.0', 'BlueOak-1.0.0', 'CC-BY-4.0'])
// One exception, by name: Miniflare, the local Workers runtime the tests and
// `wrangler dev` use, depends on sharp, whose prebuilt libvips binaries are
// LGPL-3.0-or-later. They're never bundled into the Worker or distributed
// with UpNext, so the project's MIT OR Apache-2.0 isn't affected.
const LGPL_TOOL = /^@img\/sharp-/

// An SPDX expression passes when one side of each OR passes and every part
// of each AND does.
function allowed(name, expr) {
  const ok = (id) => TOOLS.has(id) || (id === 'LGPL-3.0-or-later' && LGPL_TOOL.test(name))
  return expr
    .replace(/[()]/g, '')
    .split(/\s+OR\s+/)
    .some((side) => side.split(/\s+AND\s+/).every((id) => ok(id.trim())))
}

const counts = new Map()
const bad = []
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path) continue
  const name = path.replace(/^.*node_modules\//, '')
  const licence = typeof pkg.license === 'string' ? pkg.license : null
  const key = `${licence ?? '(none)'}${pkg.dev ? '' : ' (not dev)'}`
  counts.set(key, (counts.get(key) ?? 0) + 1)
  // Everything here must be a development dependency: the Worker ships none.
  if (!pkg.dev || !licence || !allowed(name, licence)) bad.push(`${name}@${pkg.version}: ${licence ?? 'no licence'}${pkg.dev ? '' : ', not a dev dependency'}`)
}

for (const [key, n] of [...counts].sort((a, b) => b[1] - a[1])) console.log(`${String(n).padStart(4)}  ${key}`)
if (bad.length) {
  console.error(`\nNot on the allowed list (services/verdicts/scripts/licenses.mjs):\n  ${bad.join('\n  ')}`)
  process.exit(1)
}
console.log(`\n${Object.keys(lock.packages).length - 1} packages, all allowed.`)
