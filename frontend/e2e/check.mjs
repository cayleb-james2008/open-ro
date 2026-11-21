// check.mjs — `npm run check:ui [-- --skip-build --shots --lints]` from frontend/.
// Builds the frontend and the binaries, starts Open-Ro on free ports with a fresh game, the mock Studio and a stub
// model, runs the Quest gate and the worst-case replay (each on fresh data), then stops everything it started. Fails unless every check passes and each script ran its full count. Output: frontend/e2e/.out/.
import { build, start, stop, results, BASE, E2E, OUT } from './fixture.mjs'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'
const EXPECT = { gate: 53, replay: 10 }
const args = new Set(process.argv.slice(2))
await stop() // anything an interrupted run left behind
for (const sig of ['SIGINT', 'SIGTERM']) process.on(sig, () => stop().finally(() => process.exit(130)))
fs.rmSync(OUT, { recursive: true, force: true })
let crashed = null
try {
  if (!args.has('--skip-build')) build()
  for (const step of ['gate', 'replay', ...(args.has('--shots') ? ['shots'] : [])]) {
    console.log(`\n== ${step} on ${await start({ fresh: true })}`)
    await import(`./${step}.mjs`)
  }
  if (args.has('--lints')) {
    console.log(`\n== lints (report only) on ${await start({ fresh: true })}`)
    execFileSync('bash', [`${E2E}/lints.sh`, BASE, `${OUT}/lints`], { stdio: 'inherit' })
  }
} catch (e) { crashed = e } finally { await stop() }
if (crashed) { console.error('\ncheck:ui stopped:', crashed); process.exit(1) }
let bad = 0
console.log('\n== check:ui')
for (const [name, want] of Object.entries(EXPECT)) {
  const r = results.find(x => x.name === name)
  const fine = r && r.pass === r.total && r.total === want
  if (!fine) bad++
  console.log(`${fine ? 'ok  ' : 'FAIL'} ${name} ${r ? `${r.pass}/${r.total}` : 'did not run'}${r && r.total !== want ? ` (expected ${want} checks)` : ''}`)
}
process.exit(bad ? 1 : 0)
