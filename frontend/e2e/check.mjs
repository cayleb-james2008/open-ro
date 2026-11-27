// check.mjs — `npm run check:ui [-- --skip-build --shots --lints]` from frontend/.
// Runs role-coverage, motion-source, single-file frontend build, Rust binary build, full E2E gate, and worst-case replay.
import { build, start, stop, results, BASE, E2E, OUT } from './fixture.mjs'
import { execFileSync } from 'node:child_process'
import fs from 'node:fs'

const args = new Set(process.argv.slice(2))
await stop()
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

const MIN_EXPECT = { gate: 236, replay: 11 }
let bad = 0
console.log('\n== check:ui')
for (const [name, minWant] of Object.entries(MIN_EXPECT)) {
  const r = results.find(x => x.name === name)
  const fine = r && r.pass === r.total && r.total >= minWant
  if (!fine) bad++
  console.log(`${fine ? 'ok  ' : 'FAIL'} ${name} ${r ? `${r.pass}/${r.total}` : 'did not run'}${r && r.total < minWant ? ` (expected >= ${minWant} checks)` : ''}`)
}
process.exit(bad ? 1 : 0)
