// replay.mjs: Open-Ro's share of the step 9 worst cases, fed through browser page.route mocks only (odd, long and
// malformed data must not blank or break the page). Run through `npm run check:ui` on fresh fixture data; shots to .out/replay.
import { BASE, OUT as E2E_OUT, launch, report } from './fixture.mjs'
import fs from 'node:fs'
const OUT = `${E2E_OUT}/replay`
fs.mkdirSync(OUT, { recursive: true })
const b = await launch()
const res = []
const ok = (n, p, d = '') => { res.push({ n, p: !!p, d }); console.log((p ? 'PASS ' : 'FAIL ') + n + (d ? '  ' + d : '')) }
const LONG = 'x'.repeat(260)
const clipped = p => p.evaluate(() => [...document.querySelectorAll('body *')].filter(e => { const r = e.getBoundingClientRect(); if (!r.width || r.right <= innerWidth + 1) return false; for (let q = e.parentElement; q; q = q.parentElement) { const o = getComputedStyle(q).overflowX; if (o !== 'visible') return false } return true }).length)
const textOverflow = (p, sel) => p.evaluate(s => [...document.querySelectorAll(s)].filter(e => e.scrollWidth > e.clientWidth + 1).length, sel)
async function page(w, url, routes, theme = 'light') {
  const ctx = await b.newContext({ viewport: { width: w, height: w < 768 ? 844 : 900 } })
  const p = await ctx.newPage(); p.errs = []
  p.on('pageerror', e => p.errs.push(e.message))
  for (const [pat, fn] of routes) await p.route(pat, fn)
  await p.goto(url); await p.waitForTimeout(2200)
  return { ctx, p }
}
const json = (r, body, status = 200) => r.fulfill({ status, contentType: 'application/json', body: typeof body === 'string' ? body : JSON.stringify(body) })
const patch = (fn) => async r => { const res = await r.fetch(); const j = await res.json(); r.fulfill({ response: res, body: JSON.stringify(fn(j)) }) }

for (const w of [1440, 390]) {
  { const { ctx, p } = await page(w, BASE, [['**/api/ideas', r => r.request().method() === 'POST' ? json(r, { ideas: [{ id: 'bare', title: 'Bare idea' }], built_titles: [] }) : r.continue()]])
    ok(`openro ${w}: an idea without pitch or skills does not blank the page`, !p.errs.length && await p.locator('#stage-title').count() === 1, p.errs.join('|'))
    if (await p.locator('section[aria-labelledby=stage-title] button.card-stock').count()) { await p.locator('section[aria-labelledby=stage-title] button.card-stock').first().click(); await p.waitForTimeout(300); ok(`openro ${w}: its Describe stage renders`, !p.errs.length && (await p.locator('#stage-title').innerText()) === 'Bare idea') }
    await ctx.close() }
  { const thread = ['You: Build this — Lava Obby', 'OpenRO: Building it now — these are my built-in steps, no model call involved.', 'Wrote ServerScriptService.' + LONG + '.', 'Playtest failed: ' + LONG]
    const { ctx, p } = await page(w, BASE, [['**/api/thread', r => json(r, { thread })]])
    ok(`openro ${w}: long unbroken playtest text stays in the card`, (await clipped(p)) === 0 && (await textOverflow(p, 'section[aria-labelledby=stage-title] p, section[aria-labelledby=stage-title] li')) === 0); await p.screenshot({ path: `${OUT}/openro-long-${w}.png` }); await ctx.close() }
  // the counts read how many changes the last build made and how many pieces the game has: one of each, and a four-digit game
  const built = ['You: Build this — Lava Obby', 'OpenRO: Building it now — these are my built-in steps, no model call involved.', 'Wrote ServerScriptService.Only.', 'Playtest passed.', 'OpenRO: Built “Lava Obby”.']
  const ideas = r => r.request().method() === 'POST' ? json(r, { ideas: [], built_titles: ['Lava Obby'] }) : r.continue()
  const stage = (p, i) => p.locator('nav[aria-label="Quest stages"] button').nth(i).click().then(() => p.waitForTimeout(500))
  // what a person reads: the count (its screen-reader text) and the words after it, and whether they sit on one line
  const reading = (p, t) => p.getByText(t).first().evaluate(e => { const cs = getComputedStyle(e); const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.4; const own = [...e.childNodes].filter(c => c.nodeType === 3).map(c => c.textContent).join('').replace(/\s+/g, ' ').trim(); return { oneLine: e.getBoundingClientRect().height < lh * 1.6, text: ((e.querySelector('.sr-only')?.textContent ?? '') + ' ' + own).trim() } }).catch(() => null)
  { const { ctx, p } = await page(w, BASE, [['**/api/thread', r => json(r, { thread: built })], ['**/api/ideas', ideas], ['**/api/place-tree', r => json(r, { scripts: { 'ServerScriptService.Only': '-- x' }, instances: {} })]])
    await stage(p, 2); const one = await reading(p, 'change made in Studio')
    await stage(p, 4); const piece = await reading(p, 'piece in your game')
    ok(`openro ${w}: one change and one piece read in the singular, on their line`, !p.errs.length && one?.text === '1 change made in Studio' && one.oneLine && piece?.text === '1 piece in your game' && piece.oneLine, p.errs.join('|') + ' ' + JSON.stringify({ one, piece })); await ctx.close() }
  { const many = Object.fromEntries(Array.from({ length: 1234 }, (_, i) => ['Workspace.Part' + i, {}]))
    const { ctx, p } = await page(w, BASE, [['**/api/thread', r => json(r, { thread: built })], ['**/api/ideas', ideas], ['**/api/place-tree', r => json(r, { scripts: {}, instances: many })]])
    await stage(p, 4); const big = await reading(p, 'pieces in your game')
    ok(`openro ${w}: a four-digit piece count stays on its line and in the card`, !p.errs.length && big?.text === '1234 pieces in your game' && big.oneLine && (await clipped(p)) === 0, p.errs.join('|') + ' ' + JSON.stringify(big)); await p.screenshot({ path: `${OUT}/openro-bigcount-${w}.png` }); await ctx.close() }
}
await b.close()
report('replay', res)
