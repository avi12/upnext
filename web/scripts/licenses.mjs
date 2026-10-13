// Checks the licence of every npm package in package-lock.json (the
// `license` field npm copies from each package): what ships in the UI's
// bundle must be permissive, and the build tools may also be weak copyleft.
// Prints a count per licence; exits 1 on anything else, or a package with
// no licence. `npm run licenses`; CI runs it.
import { readFileSync } from 'node:fs'

const lock = JSON.parse(readFileSync(new URL('../package-lock.json', import.meta.url), 'utf8'))

/** Fine in the bundle. The fonts are OFL-1.1 and their licence files ship with @fontsource. */
const SHIPPED = new Set(['MIT', 'MIT-0', 'ISC', '0BSD', 'BSD-2-Clause', 'BSD-3-Clause', 'Apache-2.0', 'CC0-1.0', 'OFL-1.1'])
/** Also fine for tools that only run at build or test time (lightningcss, argparse, lru-cache). */
const TOOLS = new Set([...SHIPPED, 'MPL-2.0', 'Python-2.0', 'BlueOak-1.0.0', 'CC-BY-4.0'])

/** An SPDX expression of ORs (`(MIT OR CC0-1.0)`) passes when one side does. */
function allowed(expr, ok) {
  return expr
    .replace(/[()]/g, '')
    .split(/\s+OR\s+/)
    .some((id) => ok.has(id.trim()))
}

const counts = new Map()
const bad = []
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path) continue
  const name = path.replace(/^.*node_modules\//, '')
  const licence = typeof pkg.license === 'string' ? pkg.license : null
  const kind = pkg.dev ? 'dev' : 'shipped'
  const key = `${licence ?? '(none)'} (${kind})`
  counts.set(key, (counts.get(key) ?? 0) + 1)
  if (!licence || !allowed(licence, pkg.dev ? TOOLS : SHIPPED)) bad.push(`${name}@${pkg.version}: ${licence ?? 'no licence'} (${kind})`)
}

for (const [key, n] of [...counts].sort((a, b) => b[1] - a[1])) console.log(`${String(n).padStart(4)}  ${key}`)
if (bad.length) {
  console.error(`\nNot on the allowed list (web/scripts/licenses.mjs):\n  ${bad.join('\n  ')}`)
  process.exit(1)
}
console.log(`\n${Object.keys(lock.packages).length - 1} packages, all allowed.`)
