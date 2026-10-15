// An OpenAI-compatible stand-in for the model, for the smoke test: it picks
// the first candidates the prompt lists ("#12 Name | …") that the request
// didn't turn down, then the next ones as spares, like the fake in src/server/api_tests.rs. Every reply used
// 1,000 tokens in and 50 out.
import { createServer } from 'node:http'
import { verdictsService } from './fake-verdicts.mjs'

const port = Number(process.env.FAKE_LLM_PORT ?? 8799)

function reply(content) {
  return {
    model: 'fake-model',
    usage: { prompt_tokens: 1000, completion_tokens: 50, total_tokens: 1050 },
    choices: [{ finish_reason: 'stop', message: { role: 'assistant', content: JSON.stringify(content) } }],
  }
}

// What a model would say about the games the README and store screenshots
// show (readme-shot.spec.ts), so the shots don't carry a placeholder: a model
// writes one or two sentences on why each pick fits the request and the
// player. Every other game gets a placeholder.
const REASONS = {
  Unpacking: 'Calm and wordless: you learn a life from the things in the boxes, and a few moves fill an hour.',
  'Citizen Sleeper': 'A quiet, tender story told a day at a time, so an hour ends at a natural stop.',
  'Return to Monkey Island': 'Gentle puzzles and good jokes, with chapters that break an evening into easy pieces.',
  'A Short Hike': 'Nothing in it hurries you, and you can see the summit in one sitting.',
  'Baba Is You': 'Each level is a few minutes of thinking, so you can stop after any one of them.',
}

function answer(body) {
  const prompt = body.messages?.[1]?.content ?? ''
  const schema = body.response_format?.json_schema?.schema?.properties ?? {}
  // Turned-down games stay in the list, named under "Not this time"; a
  // model leaves them out, and so does this one ("Curate around this" puts
  // the drawn game, which its words match, at the top).
  const turnedDown = new Set([...(prompt.match(/I turned down: (.*)\. Don't pick these\./)?.[1] ?? '').matchAll(/#(\d+)/g)].map((m) => Number(m[1])))
  const candidates = [...prompt.matchAll(/^#(\d+) ([^|[\n]+)/gm)]
    .map((m) => ({ id: Number(m[1]), name: m[2].trim() }))
    .filter((c) => !turnedDown.has(c.id))
  if (schema.pick_id) return reply({ pick_id: candidates[0]?.id ?? null, reason: 'Shorter, and you are in the mood for it.' })
  const n = schema.picks?.maxItems ?? 3
  const spares = schema.spares?.maxItems ?? 0
  const games = candidates.map((c) => ({ ...c, reason: REASONS[c.name] ?? 'Fits the request.' }))
  return reply({ picks: games.slice(0, n), spares: games.slice(n, n + spares) })
}

createServer((req, res) => {
  let raw = ''
  req.on('data', (chunk) => (raw += chunk))
  req.on('end', () => {
    if (req.method === 'GET' && req.url === '/health') {
      res.writeHead(200).end('ok')
      return
    }
    if (verdictsService(req, res, raw)) return
    if (req.method !== 'POST' || !req.url?.endsWith('/chat/completions')) {
      res.writeHead(404).end()
      return
    }
    res.writeHead(200, { 'content-type': 'application/json' }).end(JSON.stringify(answer(JSON.parse(raw))))
  })
}).listen(port, '127.0.0.1', () => console.log(`fake model on ${port}`))
