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
}
await b.close()
report('replay', res)
