// shots.mjs: Open-Ro Quest at 1440x900 and 390x844, light and dark, viewport + full page, to .out/shots.
// `npm run check:ui -- --shots` takes them on fresh fixture data.
import { BASE, OUT as E2E_OUT, launch } from './fixture.mjs'
import fs from 'node:fs'
const OUT = `${E2E_OUT}/shots`, TAG = 'ui', URL = BASE
fs.mkdirSync(OUT, { recursive: true })
const b = await launch({ args: ['--no-sandbox'] })
const errs = []
for (const scheme of ['light', 'dark']) for (const [vn, w, h] of [['1440', 1440, 900], ['390', 390, 844]]) {
  const ctx = await b.newContext({ viewport: { width: w, height: h }, colorScheme: scheme, deviceScaleFactor: 1 })
  await ctx.addInitScript((s) => { try { localStorage.setItem('openro-theme', s) } catch {} }, scheme)
  const p = await ctx.newPage()
  p.on('console', m => m.type() === 'error' && errs.push(`${vn}-${scheme}: ${m.text()}`))
  p.on('pageerror', e => errs.push(`${vn}-${scheme}: ${e.message}`))
  await p.goto(URL, { waitUntil: 'load' }); await p.waitForTimeout(1500)
  await p.screenshot({ path: `${OUT}/${TAG}-${vn}-${scheme}.png` })
  await p.addStyleTag({ content: '.h-dvh{height:auto!important}.overflow-hidden,.overflow-y-auto{overflow:visible!important}.min-h-0{min-height:auto!important}' })
  await p.waitForTimeout(300)
  await p.screenshot({ path: `${OUT}/${TAG}-${vn}-${scheme}-full.png`, fullPage: true })
  await ctx.close()
}
await b.close()
console.log(errs.length ? errs.join('\n') : 'no console errors')
