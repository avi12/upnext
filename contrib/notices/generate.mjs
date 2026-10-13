// Writes THIRD_PARTY_NOTICES.txt, the licences and notices of everything
// built into UpNext's programs and web UI, and the Android app's own
// android/app/src/main/assets/third_party_notices.txt. Every release
// artefact carries the first (docs/dev/RELEASE.md, "Third-party notices"),
// and the programs serve it at /third-party-notices.txt, which Settings ›
// About links to.
//
//   node contrib/notices/generate.mjs           write both files
//   node contrib/notices/generate.mjs --check   fail if either is out of date
//
// It needs cargo-about 0.9.2 (CARGO_ABOUT; .github/workflows/notices.yml
// installs the same) with
// the crates downloaded (`cargo fetch --locked`), and web/node_modules
// (`cd web && npm ci`), and Node 22.18 or newer to read credits.ts. Where
// things come from:
//
// - The Rust crates: `cargo about generate` with contrib/notices/about.toml
//   lists the crates the release targets build into the two programs (no
//   build or dev dependencies) and the licence it picked from each crate's
//   SPDX expression. Each crate's own licence, copyright and notice files go
//   in as they are, apart from the text of a licence the crate offers as an
//   alternative and wasn't picked under; cargo-about's standard text stands
//   in for a licence the crate has no file for. Where that text is a
//   template ("Copyright (c) <year> <copyright holders>"), the crate's real
//   copyright lines come from contrib/notices/copyrights.txt, and a crate
//   that isn't there stops the script. UpNext's own crate is left out: its
//   licences are LICENSE-MIT and LICENSE-APACHE.
// - The web UI: every package web/package-lock.json doesn't mark as a
//   development one, plus the build tools whose own code ends up in the
//   bundle (BUNDLED_TOOLS), with the licence files in web/node_modules. The
//   fonts are three of those packages, each with the OFL and its copyright.
// - Kept by hand (HAND): Microsoft's WebView2 loader, which the Windows
//   programs link statically, and tini, in the container image. Each is
//   pinned, and the check fails when what it was checked against changes.
// - The game data sources: web/src/lib/credits.ts, which Settings › About
//   shows too.
// - The Android app: its libraries (ANDROID), with the versions from
//   android/gradle/libs.versions.toml.
//
// Identical licence texts are printed once, with every package that uses
// them. The output depends only on the lock files, these inputs and the
// cargo-about version, so CI regenerates it and fails on any difference.
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, readdirSync, readFileSync, statSync, writeFileSync } from 'node:fs'
import { dirname, join, relative } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const ROOT = join(dirname(fileURLToPath(import.meta.url)), '..', '..')
const HERE = join(ROOT, 'contrib', 'notices')
const OUT = join(ROOT, 'THIRD_PARTY_NOTICES.txt')
const ANDROID_OUT = join(ROOT, 'android', 'app', 'src', 'main', 'assets', 'third_party_notices.txt')
const check = process.argv.includes('--check')

/** The cargo-about the output was made with. Another version may word a
 * standard licence text differently, so it changes only together with a
 * regeneration, here and in .github/workflows/notices.yml. */
const CARGO_ABOUT = '0.9.2'

/** A package's own licence, copyright and notice files, by name. */
const LICENCE_FILE = /^(licen[cs]e|copying|copyright|notice|unlicense)/i
/** SPDX metadata about the licences, not a licence text. */
const NOT_A_TEXT = /\.(spdx|json|html?)$/i

/** Build tools from package-lock.json's development packages whose code
 * goes into the built UI: Vite's module preloading helper, and Tailwind's
 * base styles. */
const BUNDLED_TOOLS = ['vite', 'tailwindcss']

/** What isn't in a lock file, kept by hand. `pin` is checked on every run,
 * so a change to what an entry describes fails until someone looks again. */
const HAND = [
  {
    name: 'Microsoft Edge WebView2 loader (WebView2LoaderStatic.lib)',
    version: '1.0.3800.47',
    where: 'Linked into upnext.exe and upnext-app.exe on Windows by webview2-com-sys, which carries the library from Microsoft\'s Microsoft.Web.WebView2 NuGet package of this version.',
    licence: 'BSD-3-Clause-style licence of the Microsoft.Web.WebView2 package',
    url: 'https://www.nuget.org/packages/Microsoft.Web.WebView2/1.0.3800.47',
    files: ['webview2/LICENSE.txt', 'webview2/NOTICE.txt'],
    // The licence and notice are the NuGet package's own LICENSE.txt and
    // NOTICE.txt. Its build/native/x64/WebView2LoaderStatic.lib is
    // byte-for-byte the crate's x64/WebView2LoaderStatic.lib, checked here.
    pin: () => {
      const dir = crateDir('webview2-com-sys', '0.39.1')
      const lib = join(dir, 'x64', 'WebView2LoaderStatic.lib')
      const sha = createHash('sha256').update(readFileSync(lib)).digest('hex')
      if (sha !== '89c6b872783b8f6c3cedbff618adb42082d455c615453ae10cfc753f1e8f25d8')
        return `webview2-com-sys's x64/WebView2LoaderStatic.lib has changed (SHA-256 ${sha})`
    },
  },
  {
    name: 'tini',
    version: '0.19.0',
    where: 'In the container image only, as /usr/bin/tini.',
    licence: 'MIT',
    url: 'https://github.com/krallin/tini/tree/v0.19.0',
    files: ['tini/LICENSE'],
    pin: () => {
      if (!readFileSync(join(ROOT, 'Dockerfile'), 'utf8').includes('/tini/releases/download/v0.19.0/'))
        return "the Dockerfile no longer downloads tini 0.19.0"
    },
  },
]

/** The Android app's libraries (android/app/build.gradle.kts), all under
 * the Apache License 2.0. `alias` names the entry in libs.versions.toml
 * whose version is used; the others are what those bring with them. Every
 * library in libs.versions.toml but the test-only ones needs an entry.
 * What they bring isn't in libs.versions.toml, and this script runs where
 * there is no Android SDK, so the last entry is kept by hand: after a
 * change to libs.versions.toml, compare it with every library in
 * `./gradlew :app:dependencies --configuration releaseRuntimeClasspath`. */
const ANDROID = [
  { name: 'AndroidX Activity', alias: 'androidx-activity', copyright: 'The Android Open Source Project' },
  { name: 'AndroidX Core', alias: 'androidx-core', copyright: 'The Android Open Source Project' },
  { name: 'kotlinx.serialization', alias: 'kotlinx-serialization-json', copyright: 'JetBrains s.r.o. and contributors' },
  { name: 'Kotlin standard library', version: 'kotlin', copyright: 'JetBrains s.r.o. and Kotlin Programming Language contributors' },
  { name: 'zxing-android-embedded', alias: 'zxing-android-embedded', copyright: 'JourneyApps' },
  // The version zxing-android-embedded asks for: when that changes, look
  // again with ./gradlew :app:dependencies. Its encoders hold code from
  // Barcode4J, whose notice ZXing keeps in its repository's NOTICE (tag
  // zxing-3.4.1) rather than in the jar, so it is passed on from here.
  {
    name: 'ZXing core',
    fixed: '3.4.1',
    with: ['zxing-android-embedded', '4.3.0'],
    copyright: 'ZXing authors',
    notice: 'zxing/NOTICE',
  },
  {
    name: 'What AndroidX and Kotlin bring with them: AndroidX Annotation, Arch Core, Collection, Concurrent, Interpolator, Lifecycle, SavedState, Startup, Profile Installer, Tracing and VersionedParcelable, kotlinx.coroutines, JetBrains\' Java annotations, and Guava\'s ListenableFuture',
    fixed: 'in the versions Gradle resolves (./gradlew :app:dependencies)',
    copyright: 'The Android Open Source Project, JetBrains s.r.o. and contributors, and The Guava Authors',
  },
]
const ANDROID_TEST_ONLY = new Set(['junit'])

/** Plain code-point order, the same on every machine and locale. */
const cmp = (a, b) => (a < b ? -1 : a > b ? 1 : 0)
/** Versions part by part, numbers as numbers: 0.9.1 before 0.10.0. */
function cmpVersion(a, b) {
  const pa = a.split(/(\d+)/)
  const pb = b.split(/(\d+)/)
  for (let i = 0; i < Math.max(pa.length, pb.length); i++) {
    const x = pa[i] ?? ''
    const y = pb[i] ?? ''
    const d = i % 2 ? Number(x || -1) - Number(y || -1) : cmp(x, y)
    if (d) return d
  }
  return 0
}

function fail(message) {
  console.error(`generate.mjs: ${message}`)
  process.exit(1)
}

/** Line endings, trailing spaces and blank lines at either end don't make
 * two texts different. */
function normalise(text) {
  return text
    .replace(/^\uFEFF/, '')
    .replace(/\r\n?/g, '\n')
    .split('\n')
    .map((l) => l.replace(/[ \t]+$/, ''))
    .join('\n')
    .replace(/^\n+|\n+$/g, '')
}

/** The SPDX id of a file that is one of the common licences' texts, from
 * how it starts; null for anything else (copyright files, notices,
 * third-party notices, several licences in one file). */
function standardLicence(text) {
  const start = normalise(text).slice(0, 600).replace(/\s+/g, ' ').trim()
  if (/^(The )?Apache License,? Version 2\.0/i.test(start) || /^Apache License Version 2\.0, January 2004/i.test(start))
    return /LLVM Exceptions/.test(text) ? null : 'Apache-2.0'
  if (/^Boost Software License/i.test(start)) return 'BSL-1.0'
  if (/^This is free and unencumbered software released into the public domain/i.test(start)) return 'Unlicense'
  if (/^(ISC License )?(\(ISC\) )?Copyright [^]*?Permission to use, copy, modify, and\/or distribute this software for any purpose with or without fee is hereby granted, provided that/i.test(start) && text.length < 1500)
    return 'ISC'
  if (/^(zlib License )?(\(zlib\) )?Copyright [^]*?This software is provided 'as-is'/i.test(start)) return 'Zlib'
  return null
}

function licenceFiles(dir) {
  return readdirSync(dir)
    .filter((f) => LICENCE_FILE.test(f) && !NOT_A_TEXT.test(f) && statSync(join(dir, f)).isFile())
    .sort()
}

/** The placeholder in a standard licence text's copyright line, which
 * cargo-about leaves for the copyright holders to fill in. The Apache
 * License's "[yyyy] [name of copyright owner]" isn't one: that is the
 * licence's own advice on applying it. */
const PLACEHOLDER = /<(year|copyright holders?|owner|organization)>/i

/** The copyright lines of crates whose licence cargo-about can give only as
 * a template, by crate name, from contrib/notices/copyrights.txt. */
const copyrights = new Map()
for (const [i, line] of readFileSync(join(HERE, 'copyrights.txt'), 'utf8').split('\n').entries()) {
  if (!line.trim() || line.startsWith('#')) continue
  const m = /^([A-Za-z0-9_-]+): (Copyright .+)$/.exec(line.trimEnd())
  if (!m) fail(`contrib/notices/copyrights.txt line ${i + 1} isn't "crate: Copyright …"`)
  if (!copyrights.has(m[1])) copyrights.set(m[1], [])
  copyrights.get(m[1]).push(m[2])
}
const copyrightsUsed = new Set()

/** Two texts compared for a sentence, whatever their line breaks and
 * indents. */
const flat = (text) => text.replace(/\s+/g, ' ')

/** cargo-about's standard text for a crate with no file for it. A template
 * is dropped when one of the crate's own files already has the licence
 * (with its copyright), and otherwise gets the crate's lines from
 * copyrights.txt; with neither the script stops. */
function standardText(text, id, user, name, ownTexts) {
  if (!PLACEHOLDER.test(text)) return text
  const lines = text.split('\n')
  const at = lines.findIndex((l) => PLACEHOLDER.test(l))
  const body = lines.slice(at + 1).find((l) => l.trim())
  if (body && ownTexts.some((t) => flat(t).includes(flat(body.trim()).slice(0, 60)))) return null
  const own = copyrights.get(name)
  if (!own) {
    fail(
      `cargo-about has only a template for ${user}'s ${id} licence ("${lines[at].trim()}"), and the crate has no file with it. ` +
        'Find its copyright lines in its repository and add them to contrib/notices/copyrights.txt, saying where they came from.',
    )
  }
  copyrightsUsed.add(name)
  const indent = lines[at].match(/^\s*/)[0]
  return [...lines.slice(0, at), ...own.map((c) => indent + c), ...lines.slice(at + 1)].join('\n')
}

// The crates, from cargo-about.
const aboutVersion = spawnSync('cargo', ['about', '--version'], { encoding: 'utf8' })
const installed = /cargo-about (\S+)/.exec(aboutVersion.stdout ?? '')?.[1]
if (installed !== CARGO_ABOUT) {
  fail(
    `this needs cargo-about ${CARGO_ABOUT}, not ${installed ?? 'none'}: ` +
      `cargo install --locked cargo-about --version ${CARGO_ABOUT} --features cli`,
  )
}
const about = spawnSync(
  'cargo',
  ['about', 'generate', '--format', 'json', '--frozen', '--fail', '-m', join(ROOT, 'Cargo.toml'), '-c', join(HERE, 'about.toml')],
  { encoding: 'utf8', maxBuffer: 256 * 1024 * 1024 },
)
if (about.error || about.status !== 0) {
  fail(
    `cargo about failed${about.error ? `: ${about.error.message}` : ''}\n${about.stderr ?? ''}\n` +
      'Download the crates with `cargo fetch --locked`.',
  )
}
const aboutJson = JSON.parse(about.stdout)
const crateDirs = new Map(aboutJson.crates.map((c) => [`${c.package.name} ${c.package.version}`, dirname(c.package.manifest_path)]))
function crateDir(name, version) {
  const dir = crateDirs.get(`${name} ${version}`)
  if (!dir) fail(`${name} ${version} isn't among the crates any more: check the hand-kept entry that names it`)
  return dir
}

/** Every distinct licence text, by its normalised text. */
const texts = new Map()
function addText(text, label, user) {
  const key = normalise(text)
  if (!key) return null
  let t = texts.get(key)
  if (!t) texts.set(key, (t = { text: key, labels: new Set(), users: new Set() }))
  t.labels.add(label)
  t.users.add(user)
  return t
}

// cargo-about's licence for each crate, with the file it came from, or its
// standard text where the crate has no file for it.
const chosen = new Map()
for (const l of aboutJson.licenses) {
  for (const u of l.used_by) {
    const key = `${u.crate.name} ${u.crate.version}`
    if (!chosen.has(key)) chosen.set(key, [])
    chosen.get(key).push(l)
  }
}

// UpNext itself, the one crate not from crates.io, has its own licences.
const thirdParty = aboutJson.crates.filter((c) => c.package.source)
const crates = []
for (const c of [...thirdParty].sort((a, b) => cmp(a.package.name, b.package.name) || cmpVersion(a.package.version, b.package.version))) {
  const { name, version, authors } = c.package
  const user = `${name} ${version}`
  const dir = dirname(c.package.manifest_path)
  const refs = new Set()
  const picked = (chosen.get(user) ?? []).map((l) => l.id)
  const own = licenceFiles(dir)
  const ownTexts = own.map((f) => readFileSync(join(dir, f), 'utf8'))
  for (const [i, f] of own.entries()) {
    const text = ownTexts[i]
    // The text of a licence the crate offers as an alternative, and which
    // cargo-about didn't pick, isn't the one UpNext uses the crate under.
    const id = standardLicence(text)
    if (id && / OR /.test(c.license) && c.license.split(/[\s()]+/).includes(id) && !picked.includes(id)) continue
    refs.add(addText(text, f, user))
  }
  for (const l of chosen.get(user) ?? []) {
    // A file cargo-about found below the crate's top folder is read too, and
    // its standard text stands in for a licence the crate has no file for.
    if (l.source_path && !own.includes(relative(dir, l.source_path))) refs.add(addText(readFileSync(l.source_path, 'utf8'), l.id, user))
    if (!l.source_path) {
      const text = standardText(l.text, l.id, user, name, ownTexts)
      if (text) refs.add(addText(text, l.id, user))
    }
  }
  refs.delete(null)
  if (refs.size === 0) fail(`no licence text for the crate ${user}`)
  crates.push({ name, version, licence: c.license, refs, authors: own.length === 0 ? authors : [], link: `https://crates.io/crates/${name}/${version}` })
}
for (const name of copyrights.keys()) {
  if (!copyrightsUsed.has(name)) fail(`contrib/notices/copyrights.txt has ${name}, which doesn't need lines from it (it has a licence file, or cargo-about's text isn't a template): remove them`)
}

// The web UI's packages.
const lock = JSON.parse(readFileSync(join(ROOT, 'web', 'package-lock.json'), 'utf8'))
const packages = []
for (const [path, pkg] of Object.entries(lock.packages)) {
  if (!path) continue
  const name = path.replace(/^.*node_modules\//, '')
  if (pkg.dev && !BUNDLED_TOOLS.includes(name)) continue
  const dir = join(ROOT, 'web', path)
  if (!existsSync(dir)) fail(`web/${path} isn't there: run npm ci in web/ first`)
  const installed = JSON.parse(readFileSync(join(dir, 'package.json'), 'utf8')).version
  if (installed !== pkg.version) fail(`web/${path} is ${installed}, but package-lock.json says ${pkg.version}: run npm ci in web/`)
  const user = `${name} ${pkg.version}`
  const files = licenceFiles(dir)
  if (files.length === 0) fail(`the npm package ${user} has no licence file; add its text by hand`)
  const refs = new Set(files.map((f) => addText(readFileSync(join(dir, f), 'utf8'), f, user)))
  refs.delete(null)
  packages.push({ name, version: pkg.version, licence: pkg.license, refs, tool: !!pkg.dev, link: `https://www.npmjs.com/package/${name}/v/${pkg.version}` })
}
for (const tool of BUNDLED_TOOLS) {
  if (!packages.some((p) => p.name === tool)) fail(`${tool} isn't in web/package-lock.json any more: update BUNDLED_TOOLS`)
}
packages.sort((a, b) => cmp(a.name, b.name))

// Kept by hand.
for (const h of HAND) {
  const problem = h.pin()
  if (problem) fail(`${problem}. Check ${h.name} again and update its entry in contrib/notices/generate.mjs.`)
  h.refs = new Set(h.files.map((f) => addText(readFileSync(join(HERE, f), 'utf8'), f.split('/').pop(), `${h.name} ${h.version}`)))
}

// The data sources.
const { CREDITS } = await import(pathToFileURL(join(ROOT, 'web', 'src', 'lib', 'credits.ts')).href)

// Number the texts: by licence, then by the first package that uses each.
const ordered = [...texts.values()]
  .map((t) => ({ ...t, label: [...t.labels].sort(cmp).join(', '), users: [...t.users].sort(cmp) }))
  .sort((a, b) => cmp(a.label, b.label) || cmp(a.users[0], b.users[0]) || cmp(a.text, b.text))
const number = new Map(ordered.map((t, i) => [t.text, i + 1]))
const nums = (refs) => [...refs].map((r) => number.get(r.text)).sort((a, b) => a - b).join(', ')

const RULE = '='.repeat(78)
const lines = []
const say = (...l) => lines.push(...l)
const heading = (title) => say('', '', title, '-'.repeat(title.length), '')

say(
  'UpNext: third-party notices',
  '===========================',
  '',
  'UpNext is © UpNext contributors, under the MIT licence or the Apache License 2.0,',
  'at your option (LICENSE-MIT and LICENSE-APACHE). Its programs (upnext and',
  'upnext-app) and the web UI built into them are made with the open-source',
  'packages listed here, each under its own licence. This file names every one,',
  'with its version and licence, and then gives the licence, copyright and notice',
  'texts they ask to be passed on with them, each text once with every package',
  'that uses it. The numbers in brackets are those texts.',
  '',
  'Not every copy of UpNext has all of them: the container image and builds',
  'without the app window leave out the window\'s crates (wry, tao, GTK and the',
  'rest of the desktop feature), only the Windows programs carry the Windows',
  'crates and the WebView2 loader, and only the container image carries tini.',
  'The Linux libraries the Steam copy bundles have their licences in its',
  'share/licenses folder, and the Android app lists its own libraries under',
  'Open-source licences on its first screen.',
  '',
  'Generated by contrib/notices/generate.mjs from Cargo.lock and',
  'web/package-lock.json; don\'t edit it by hand. docs/THIRD_PARTY.md describes',
  'the services UpNext talks to, and their terms.',
)

heading(`1. Rust crates (${crates.length})`)
for (const c of crates) {
  say(`${c.name} ${c.version}  ${c.licence}  [${nums(c.refs)}]`, `    ${c.link}`)
  if (c.authors.length) say(`    No licence file of its own; its authors: ${c.authors.join(', ')}`)
}

heading(`2. Web UI packages (${packages.length})`)
for (const p of packages) {
  say(`${p.name} ${p.version}  ${p.licence}  [${nums(p.refs)}]${p.tool ? '  (a build tool; some of its code is in the built UI)' : ''}`, `    ${p.link}`)
}

heading('3. Kept by hand')
for (const h of HAND) {
  say(`${h.name} ${h.version}  ${h.licence}  [${nums(h.refs)}]`, `    ${h.url}`, `    ${h.where}`)
}

heading('4. Game data')
say(
  'UpNext reads game data from these services, under their own terms; it isn\'t',
  'affiliated with any of them. docs/THIRD_PARTY.md has every service\'s terms.',
  '',
)
for (const c of CREDITS) {
  say(`${c.name} (${c.url}): ${c.what}.`)
  if (c.note) say(`    ${c.note}`)
  if (c.terms) say(`    ${c.terms.label}: ${c.terms.url}`)
}

heading(`5. Licence texts (${ordered.length})`)
for (const t of ordered) {
  say(RULE, `[${number.get(t.text)}] ${t.label}`, `Used by: ${t.users.join(', ')}`, RULE, '', t.text, '')
}
const notices = lines.join('\n').replace(/\n+$/, '') + '\n'

// The Android app's notices.
const toml = readFileSync(join(ROOT, 'android', 'gradle', 'libs.versions.toml'), 'utf8')
const section = (name) => (toml.split(/^\[/m).find((s) => s.startsWith(`${name}]`)) ?? '').split('\n').slice(1)
const versions = new Map(
  section('versions').flatMap((l) => {
    const m = /^(\S+)\s*=\s*"([^"]+)"/.exec(l)
    return m ? [[m[1], m[2]]] : []
  }),
)
const libraries = new Map(
  section('libraries').flatMap((l) => {
    const m = /^(\S+)\s*=\s*\{.*version\.ref\s*=\s*"([^"]+)"/.exec(l)
    return m ? [[m[1], versions.get(m[2])]] : []
  }),
)
for (const alias of libraries.keys()) {
  if (!ANDROID_TEST_ONLY.has(alias) && !ANDROID.some((a) => a.alias === alias))
    fail(`android/gradle/libs.versions.toml has ${alias}, which ANDROID in contrib/notices/generate.mjs doesn't list`)
}
const apache = normalise(readFileSync(join(ROOT, 'LICENSE-APACHE'), 'utf8'))
const android = [
  'UpNext for Android: open-source licences',
  '',
  'UpNext is © UpNext contributors, under the MIT licence or the Apache License',
  '2.0, at your option. The app is built with these libraries, each under the',
  'Apache License 2.0, whose text is below.',
  '',
  ...ANDROID.flatMap((a) => {
    const version = a.alias ? libraries.get(a.alias) : a.version ? versions.get(a.version) : a.fixed
    if (!version) fail(`no version for ${a.name} in android/gradle/libs.versions.toml`)
    if (a.with && libraries.get(a.with[0]) !== a.with[1])
      fail(`${a.with[0]} is no longer ${a.with[1]}: check which ${a.name} it brings, and update ANDROID in contrib/notices/generate.mjs`)
    return [`${a.name} ${version}`, `    © ${a.copyright}`]
  }),
  '',
  'The computer\'s UpNext lists what its own programs and web UI are built',
  'with in Settings › About, Third-party notices.',
  '',
  ...ANDROID.filter((a) => a.notice).flatMap((a) => [
    RULE,
    `${a.name}: its NOTICE file`,
    RULE,
    '',
    normalise(readFileSync(join(HERE, a.notice), 'utf8')),
    '',
  ]),
  RULE,
  'Apache License 2.0',
  RULE,
  '',
  apache,
  '',
].join('\n')

if (check) {
  const stale = [
    [OUT, notices],
    [ANDROID_OUT, android],
  ].filter(([file, want]) => !existsSync(file) || readFileSync(file, 'utf8') !== want)
  if (stale.length) {
    for (const [file, want] of stale) {
      const have = existsSync(file) ? readFileSync(file, 'utf8').split('\n') : []
      const w = want.split('\n')
      const at = w.findIndex((l, i) => l !== have[i])
      console.error(`${relative(ROOT, file)} is out of date; first difference at line ${at + 1}:`)
      console.error(`  committed: ${have[at] ?? '(end of file)'}`)
      console.error(`  generated: ${w[at] ?? '(end of file)'}`)
    }
    fail('run `node contrib/notices/generate.mjs` and commit the result')
  }
  console.log(`THIRD_PARTY_NOTICES.txt and the Android app's notices are current: ${crates.length} crates, ${packages.length} npm packages, ${HAND.length} by hand, ${ordered.length} texts.`)
} else {
  writeFileSync(OUT, notices)
  writeFileSync(ANDROID_OUT, android)
  console.log(`Wrote ${relative(ROOT, OUT)} (${crates.length} crates, ${packages.length} npm packages, ${HAND.length} by hand, ${ordered.length} texts, ${Math.round(notices.length / 1024)} KB) and ${relative(ROOT, ANDROID_OUT)}.`)
}
