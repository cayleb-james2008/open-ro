// fixture.mjs: build OpenRO, run it on ports 18070-18079 (or free fallback) with a fresh game,
// mock Studio and stub model, and stop only the PIDs it started. Output goes to frontend/e2e/.out.
import { chromium } from 'playwright-core'
import { execFileSync, spawn } from 'node:child_process'
import fs from 'node:fs'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

export const E2E = path.dirname(fileURLToPath(import.meta.url)), ROOT = path.resolve(E2E, '../..')
export const OUT = path.join(E2E, '.out'), RUN = path.join(OUT, 'run')
const STATE = path.join(RUN, 'ports.json'), FIX = path.join(E2E, 'fixtures')
const TARGET_DIR = process.env.CARGO_TARGET_DIR || path.join(ROOT, 'target')
const BIN = path.join(TARGET_DIR, 'debug/openro-rs')
export const CHROME = process.env.CHROME_PATH || process.env.E2E_CHROME || path.join(os.homedir(), '.cache/ms-playwright/chromium-1234/chrome-linux64/chrome')
const ZERO_GPU_ARGS = ['--disable-gpu', '--disable-gpu-compositing', '--disable-gpu-sandbox', '--disable-accelerated-2d-canvas', '--disable-accelerated-video-decode', '--num-raster-threads=1']
export const launch = (opts = {}) => chromium.launch({
  headless: true,
  executablePath: CHROME,
  ...opts,
  args: [...ZERO_GPU_ARGS, ...(opts.args || [])],
  env: { ...process.env, LIBGL_ALWAYS_SOFTWARE: '1', VK_ICD_FILENAMES: '/dev/null', ...(opts.env || {}) },
})

let st = fs.existsSync(STATE) ? JSON.parse(fs.readFileSync(STATE, 'utf8')) : null
export let BASE = st && `http://127.0.0.1:${st.app}/`

export const results = []
export function report(name, checks) {
  const pass = checks.filter(c => c.pass ?? c.p).length
  results.push({ name, pass, total: checks.length })
  console.log(`\n${name}: ${pass}/${checks.length} pass`)
  if (pass !== checks.length) process.exitCode = 1
}

export function build() {
  const cargoEnv = { ...process.env, PATH: `${os.homedir()}/.local/rust/bin:${os.homedir()}/.rust/bin:${process.env.PATH || ''}`, CARGO_TARGET_DIR: TARGET_DIR }
  const run = (cmd, args, cwd) => execFileSync(cmd, args, { cwd, env: cargoEnv, stdio: 'inherit' })
  const link = path.join(ROOT, 'rune/node_modules')
  if (!fs.existsSync(link)) fs.symlinkSync('../frontend/node_modules', link)
  run('node', ['scripts/role-coverage.mjs'], path.join(ROOT, 'frontend'))
  run('node', ['scripts/motion-source.mjs'], path.join(ROOT, 'frontend'))
  run('npm', ['run', 'build'], path.join(ROOT, 'frontend'))
  // Single-binary Rust rule: touch server.rs (which calls include_str! on assets/index.html) before cargo build
  const now = new Date()
  for (const f of fs.readdirSync(path.join(ROOT, 'src'))) if (f.endsWith('.rs')) fs.utimesSync(path.join(ROOT, 'src', f), now, now)
  run('cargo', ['build', '--offline', '--bins'], ROOT)
}

const sleep = ms => new Promise(r => setTimeout(r, ms))
const listening = port => new Promise(res => { const s = net.connect(port, '127.0.0.1'); s.once('connect', () => { s.destroy(); res(true) }); s.once('error', () => res(false)) })
const tryBind = port => new Promise(res => {
  const s = net.createServer()
  s.once('error', () => res(0))
  s.listen(port, '127.0.0.1', () => { const p = s.address().port; s.close(() => res(p)) })
})
async function pickPort(taken) {
  const min = Number(process.env.E2E_PORT_MIN || 18070)
  const max = Number(process.env.E2E_PORT_MAX || 18079)
  for (let p = min; p <= max; p++) {
    if (!taken.includes(p) && await tryBind(p)) return p
  }
  for (;;) {
    const p = await tryBind(0)
    if (p && p !== 9230 && !taken.includes(p)) return p
  }
}

function alive(name, mark) {
  const f = path.join(RUN, `${name}.pid`)
  if (!fs.existsSync(f)) return 0
  const pid = Number(fs.readFileSync(f, 'utf8'))
  try { return fs.readFileSync(`/proc/${pid}/cmdline`, 'utf8').includes(mark) ? pid : 0 } catch { return 0 }
}
async function kill(name, mark) {
  const pid = alive(name, mark)
  if (pid) {
    try { process.kill(-pid, 'SIGTERM') } catch {}
    for (let i = 0; i < 30 && alive(name, mark); i++) await sleep(100)
    if (alive(name, mark)) try { process.kill(-pid, 'SIGKILL') } catch {}
    for (let i = 0; i < 20 && alive(name, mark); i++) await sleep(100)
  }
  fs.rmSync(path.join(RUN, `${name}.pid`), { force: true })
}
function launchProc(name, cmd, args, env) {
  const log = fs.openSync(path.join(RUN, `${name}.log`), 'w')
  const child = spawn(cmd, args, { cwd: ROOT, env, detached: true, stdio: ['ignore', log, log] })
  fs.closeSync(log); child.unref()
  fs.writeFileSync(path.join(RUN, `${name}.pid`), String(child.pid))
}

export async function start({ fresh = false } = {}) {
  fs.mkdirSync(RUN, { recursive: true })
  if (!st) { const app = await pickPort([]); st = { app, stub: await pickPort([app]) }; fs.writeFileSync(STATE, JSON.stringify(st)) }
  BASE = `http://127.0.0.1:${st.app}/`
  await kill('app', BIN)
  for (let i = 0; i < 20 && await listening(st.app); i++) await sleep(200)
  if (await listening(st.app)) throw new Error(`port ${st.app} is still held`)
  if (fresh || !fs.existsSync(path.join(RUN, 'data'))) {
    fs.rmSync(path.join(RUN, 'data'), { recursive: true, force: true })
    fs.mkdirSync(path.join(RUN, 'data'))
    await kill('stub', RUN)
  }
  for (const d of ['home', 'tmp']) fs.mkdirSync(path.join(RUN, d), { recursive: true })
  const env = { PATH: '/usr/bin:/bin', HOME: path.join(RUN, 'home'), TMPDIR: path.join(RUN, 'tmp'), TZ: 'UTC', LANG: 'C.UTF-8' }
  if (!alive('stub', RUN)) {
    fs.copyFileSync(path.join(FIX, 'replies.json'), path.join(RUN, 'replies.json'))
    launchProc('stub', process.execPath, [path.join(FIX, 'stub-model.mjs'), String(st.stub), path.join(RUN, 'replies.json')], env)
    for (let i = 0; i < 30 && !await listening(st.stub); i++) await sleep(100)
  }
  launchProc('app', BIN, [], { ...env, OPENRO_PORT: String(st.app), OPENRO_DATA_DIR: path.join(RUN, 'data'), OPENRO_MCP_COMMAND: path.join(path.dirname(BIN), 'mock_studio'), OPENRO_MODEL_BASE_URL: `http://127.0.0.1:${st.stub}`, OPENRO_MODEL_API_KEY: 'stub', OPENRO_GAME_NAME: 'Skyline Obby', OPENRO_UPDATE_MODE: 'off' })
  for (let i = 0; i < 50; i++) {
    if (await fetch(BASE + 'api/status').then(r => r.ok, () => false)) return BASE
    await sleep(300)
  }
  throw new Error('OpenRO did not come up:\n' + fs.readFileSync(path.join(RUN, 'app.log'), 'utf8').slice(-800))
}
export const reset = () => start({ fresh: true })

export async function stop() {
  await kill('app', BIN); await kill('stub', RUN)
  fs.rmSync(STATE, { force: true }); st = null; BASE = null
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const cmd = process.argv[2]
  if (cmd === 'start') console.log(await start({ fresh: true }))
  else if (cmd === 'stop') await stop()
  else { console.error('usage: node e2e/fixture.mjs start|stop'); process.exit(2) }
}
