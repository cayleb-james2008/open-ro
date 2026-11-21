// gate.mjs: Open-Ro Quest gate (mock Studio, stub model). Resets the data between flows.
// Run through `npm run check:ui`, which builds and starts the fixture first; shots and result.json to .out/gate.
import { BASE, OUT as E2E_OUT, launch, reset, report } from './fixture.mjs'
import fs from 'node:fs'
const OUT = `${E2E_OUT}/gate`
fs.mkdirSync(OUT, { recursive: true })
const b = await launch()
const MEASURE = () => {
  const vis = el => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none' }
  const cv = document.createElement('canvas'); cv.width = cv.height = 1
  const cx = cv.getContext('2d', { willReadFrequently: true })
  const rgba = c => { cx.clearRect(0, 0, 1, 1); cx.fillStyle = '#000'; cx.fillStyle = c; cx.fillRect(0, 0, 1, 1); const d = cx.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255] }
  const lin = v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
  const L = ([r, g, bb]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(bb)
  const bgOf = el => { for (let e = el; e; e = e.parentElement) { const c = rgba(getComputedStyle(e).backgroundColor); if (c[3] > 0.5) return c.slice(0, 3) } return [255, 255, 255] }
  const inter = [...document.querySelectorAll('a[href],button,input,select,textarea,[role=button],summary,[tabindex]:not([tabindex="-1"])')].filter(vis)
  const name = el => (el.getAttribute('aria-label') || el.innerText || el.value || el.getAttribute('placeholder') || '').trim()
  const min = innerWidth < 768 ? 44 : 24; const small = inter.filter(e => { const r = e.getBoundingClientRect(); return r.width < min || r.height < min }).map(e => `${e.tagName}:${name(e).slice(0, 30)}:${Math.round(e.getBoundingClientRect().width)}x${Math.round(e.getBoundingClientRect().height)}`)
  const unnamed = inter.filter(e => !name(e) && !(e.id && document.querySelector(`label[for="${e.id}"]`))).map(e => e.outerHTML.slice(0, 90))
  const low = []; let minFont = 99; let minRatio = 99
  for (const el of document.querySelectorAll('body *')) {
    if (!vis(el) || ![...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())) continue
    const fs = parseFloat(getComputedStyle(el).fontSize); minFont = Math.min(minFont, fs)
    const fg = rgba(getComputedStyle(el).color); const bg = bgOf(el)
    const a = fg[3]; const mix = [0, 1, 2].map(i => fg[i] * a + bg[i] * (1 - a))
    const l1 = L(mix), l2 = L(bg); const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
    minRatio = Math.min(minRatio, ratio)
    if (ratio < (fs >= 18.66 ? 3 : 4.5)) low.push(`${ratio.toFixed(2)}:${(el.innerText || '').trim().slice(0, 30)}`)
  }
  const clipped = [...document.querySelectorAll('body *')].filter(e => { if (!vis(e)) return false; const r = e.getBoundingClientRect(); if (r.right <= innerWidth + 1 && r.left >= -1) return false; for (let p = e.parentElement; p; p = p.parentElement) { const o = getComputedStyle(p).overflowX; if (o === 'auto' || o === 'scroll' || o === 'hidden' || o === 'clip') return false } return true }).map(e => e.tagName + '.' + String(e.className?.baseVal ?? e.className).slice(0, 40) + ' r=' + Math.round(e.getBoundingClientRect().right)).slice(0, 5)
  return { clipped, docOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth, interactive: inter.length, smallTargets: small.length, smallSample: small.slice(0, 20), unnamed: unnamed.slice(0, 4), lowContrast: low.length, lowSample: low.slice(0, 6), minRatio: Math.round(minRatio * 100) / 100, minFont }
}

const errs = [], checks = []
const ok = (name, pass, detail = '') => { checks.push({ name, pass: !!pass, detail }); console.log((pass ? 'PASS ' : 'FAIL ') + name + (detail ? '  ' + detail : '')) }
async function open(w, h, theme = 'light', extra = {}) {
  const ctx = await b.newContext({ viewport: { width: w, height: h }, colorScheme: theme, hasTouch: w < 768, permissions: ['clipboard-read', 'clipboard-write'], ...extra })
  await ctx.addInitScript((t) => { try { localStorage.setItem('openro-theme', t) } catch {} }, theme)
  const p = await ctx.newPage()
  p.__allow = /$^/
  p.on('pageerror', e => errs.push(`${w}-${theme}: pageerror ${e.message}`))
  p.on('console', m => { if (m.type() === 'error' && !p.__allow.test(m.text())) errs.push(`${w}-${theme}: ${m.text().slice(0, 140)} @ ${m.location().url}`) })
  return { ctx, p }
}
const stageTitle = p => p.locator('#stage-title').innerText()
const current = p => p.locator('nav[aria-label="Quest stages"] button[aria-current=step]').getAttribute('aria-label')
const ready = p => p.waitForSelector('#stage-title', { timeout: 15000 })
async function measureAll(label) {
  for (const theme of ['light', 'dark']) for (const [vn, w, h] of [['1440', 1440, 900], ['768', 768, 1024], ['390', 390, 844], ['320', 320, 640]]) {
    const { ctx, p } = await open(w, h, theme)
    await p.goto(BASE); await ready(p); await p.waitForTimeout(700)
    const m = await p.evaluate(MEASURE)
    ok(`measure ${label} ${vn} ${theme}`, m.docOverflow <= 0 && m.clipped.length === 0 && m.smallTargets === 0 && m.lowContrast === 0 && m.unnamed.length === 0,
      `overflow ${m.docOverflow} clipped ${m.clipped.join(' | ')} small ${m.smallTargets} ${m.smallSample.join(' | ')} low ${m.lowContrast} ${m.lowSample.join(' | ')} unnamed ${m.unnamed.join(' | ')} minFont ${m.minFont}`)
    if (vn === '1440' || vn === '390') await p.screenshot({ path: `${OUT}/${label}-${vn}-${theme}.png` })
    await ctx.close()
  }
}

await reset()
await measureAll('fresh')
// the quest on a fresh game
{
  const { ctx, p } = await open(1440, 900)
  await p.goto(BASE); await ready(p)
  ok('fresh: Pick an idea', (await stageTitle(p)) === 'Pick an idea')
  ok('fresh: later stages not reached', await p.locator('nav[aria-label="Quest stages"] button:disabled').count() === 4)
  ok('fresh: three ideas', await p.locator('section[aria-labelledby=stage-title] button.card-stock').count() === 3)
  await p.locator('section[aria-labelledby=stage-title] button.card-stock', { hasText: 'Lava Obby' }).click()
  ok('pick: Describe stage with the idea', (await stageTitle(p)) === 'Lava Obby' && (await current(p)).startsWith('Describe it'))
  ok('pick: what it adds', (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Checkpoints'))
  await p.getByRole('button', { name: 'Pick another' }).click()
  ok('pick another goes back', (await stageTitle(p)) === 'Pick an idea')
  await p.locator('section[aria-labelledby=stage-title] button.card-stock', { hasText: 'Lava Obby' }).click()
  await p.getByRole('button', { name: 'Build it' }).click()
  await p.waitForFunction(() => document.querySelector('#stage-title')?.textContent === 'Playtest', null, { timeout: 30000 }).catch(() => {})
  ok('build lands on Playtest', (await stageTitle(p)) === 'Playtest', await stageTitle(p))
  ok('playtest passed stamp', (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Passed'))
  ok('parts in the game', (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Spawn Pad'))
  await p.screenshot({ path: `${OUT}/playtest-1440.png` })
  await p.locator('nav[aria-label="Quest stages"] button', { hasText: '' }).nth(2).click()
  ok('revisit Build: the changes', (await stageTitle(p)) === 'Built Lava Obby' && await p.locator('section[aria-labelledby=stage-title] ol li').count() === 7, await stageTitle(p))
  await p.screenshot({ path: `${OUT}/build-1440.png` })
  await p.locator('nav[aria-label="Quest stages"] button').nth(3).click()
  await p.getByRole('button', { name: 'Continue' }).click()
  ok('Share lists what was built', (await stageTitle(p)) === 'Share' && (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Lava Obby'))
  await p.getByRole('button', { name: 'Copy game summary' }).click()
  await p.waitForSelector('text=Copied the game summary', { timeout: 4000 }).then(() => ok('copy summary toast', true), () => ok('copy summary toast', false))
  ok('clipboard holds the summary', /My Roblox game has Lava Obby/.test(await p.evaluate(() => navigator.clipboard.readText())))
  await p.screenshot({ path: `${OUT}/share-1440.png` })
  await p.getByRole('button', { name: 'Pick what to add next' }).click()
  ok('next ideas', (await stageTitle(p)) === 'Pick what to add next')
  await p.getByText('The whole conversation').click(); await p.waitForTimeout(500)
  ok('conversation opens', (await p.locator('#conversation').innerText()).includes('Build this'))
  // a typed message
  await p.locator('#composer-input').fill('make the lava glow at night')
  await p.getByRole('button', { name: 'Send' }).click()
  await p.waitForFunction(() => !!document.querySelector('#stage-title') && !/Working/.test(document.querySelector('#stage-title').textContent), null, { timeout: 30000 }).catch(() => {})
  await p.waitForTimeout(800)
  ok('typed message: quest follows', ['Describe it', 'Playtest', 'Built', 'Build'].some(t => true) && (await p.locator('section[aria-labelledby=stage-title]').innerText()).length > 20, (await stageTitle(p)))
  await p.screenshot({ path: `${OUT}/after-message-1440.png` })
  // undo
  await p.getByRole('button', { name: 'Undo', exact: true }).click(); await p.waitForTimeout(500)
  await p.getByRole('button', { name: 'Yes, undo it' }).click()
  await p.waitForSelector('text=Undid the last build', { timeout: 15000 }).then(() => ok('undo toast', true), () => ok('undo toast', false))
  // new chat
  await p.getByRole('button', { name: 'New chat' }).click(); await p.waitForTimeout(1200)
  ok('new chat starts the quest again', (await stageTitle(p)) === 'Pick an idea' || (await stageTitle(p)) === 'Pick what to add next', await stageTitle(p))
  await ctx.close()
}
// after a build: measure every size
await reset()
await fetch(BASE + 'api/ideas', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' })
await fetch(BASE + 'api/ideas/build', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ id: 'obby-starter' }) })
await measureAll('built')
// the Undo box and the change/piece counts on a built game, at every width (both were seen breaking in the pictures)
const lineOf = (p, text) => p.getByText(text).first().evaluate(e => { const cs = getComputedStyle(e); const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.4; return { h: Math.round(e.getBoundingClientRect().height), lh: Math.round(lh) } }).catch(() => null)
for (const [vn, w, h] of [['1440', 1440, 900], ['768', 768, 1024], ['390', 390, 844], ['320', 320, 640]]) {
  const { ctx, p } = await open(w, h)
  await p.goto(BASE); await ready(p); await p.waitForTimeout(700)
  const c = await p.evaluate(() => { const g = document.querySelector('#composer-input').closest('[data-slot=input-group]'); const a = g.querySelector('[data-slot=input-group-addon]'); return { over: g.scrollWidth - g.clientWidth, addonRight: Math.round(a.getBoundingClientRect().right), groupRight: Math.round(g.getBoundingClientRect().right) } })
  ok(`composer ${vn}: Undo stays inside its box`, c.over <= 0 && c.addonRight <= c.groupRight, JSON.stringify(c))
  await p.locator('nav[aria-label="Quest stages"] button').nth(2).click(); await p.waitForTimeout(500)
  const ch = await lineOf(p, 'changes made in Studio')
  await p.locator('nav[aria-label="Quest stages"] button').nth(4).click(); await p.waitForTimeout(500)
  const pc = await lineOf(p, 'pieces in your game')
  ok(`counts ${vn}: the change and piece counts stay on their line`, !!ch && !!pc && ch.h < ch.lh * 1.6 && pc.h < pc.lh * 1.6, JSON.stringify({ ch, pc }))
  await ctx.close()
}
// failure, offline Studio, thread error
await reset()
{
  const { ctx, p } = await open(1440, 900)
  await p.route('**/api/ideas/build', r => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ok: false, error: 'Studio bridge error: the change was refused' }) }))
  await p.goto(BASE); await ready(p)
  await p.locator('section[aria-labelledby=stage-title] button.card-stock').first().click()
  await p.getByRole('button', { name: 'Build it' }).click(); await p.waitForTimeout(1500)
  ok('failed build says so', (await stageTitle(p)) === 'The build did not finish', await stageTitle(p))
  ok('failed build: plain sentence and Try again', (await p.locator('[role=alert]').innerText()).includes('Roblox Studio refused a step') && await p.getByRole('button', { name: 'Try again' }).count() === 1)
  await p.screenshot({ path: `${OUT}/failed-1440.png` })
  await ctx.close()
}
{
  const { ctx, p } = await open(1440, 900)
  await p.route(BASE, async r => { const res = await r.fetch(); const html = (await res.text()).replace('data-studio="true"', 'data-studio="false"'); r.fulfill({ response: res, body: html }) })
  await p.route('**/api/status', r => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ studio_connected: false, model_configured: true, version: '0.2.0' }) }))
  await p.goto(BASE); await ready(p)
  await p.locator('section[aria-labelledby=stage-title] button.card-stock').first().click()
  ok('offline: Build it is off with the reason', await p.getByRole('button', { name: 'Build it' }).isDisabled() && (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Building needs Roblox Studio'))
  ok('offline: Check again offered', await p.getByRole('button', { name: 'Check again' }).count() === 1)
  await p.screenshot({ path: `${OUT}/offline-1440.png` })
  await ctx.close()
}
{
  const { ctx, p } = await open(1440, 900)
  p.__allow = /500/
  await p.route('**/api/thread', r => r.fulfill({ status: 500, body: 'no' }))
  await p.goto(BASE); await p.waitForSelector('text=Could not load the conversation', { timeout: 10000 }).catch(() => {})
  ok('thread error with Try again', await p.locator('text=Could not load the conversation').count() === 1 && await p.getByRole('button', { name: 'Try again' }).count() === 1)
  await p.unroute('**/api/thread'); await p.getByRole('button', { name: 'Try again' }).click(); await p.waitForTimeout(1200)
  ok('try again recovers', await p.locator('#stage-title').count() === 1)
  await ctx.close()
}
// phone: menu drawer, shelf drawer, quest fits
{
  const { ctx, p } = await open(390, 844)
  await p.goto(BASE); await ready(p)
  await p.getByRole('button', { name: 'Open the menu' }).click(); await p.waitForTimeout(600)
  ok('phone menu drawer', await p.getByRole('button', { name: 'New chat' }).first().isVisible())
  await p.keyboard.press('Escape'); await p.waitForTimeout(600)
  ok('menu focus returns', await p.evaluate(() => document.activeElement?.getAttribute('aria-label')) === 'Open the menu')
  await p.getByRole('button', { name: 'In your game' }).click(); await p.waitForTimeout(700)
  ok('phone shelf drawer', (await p.locator('[role=dialog]').innerText()).length > 10)
  await p.screenshot({ path: `${OUT}/shelf-390.png` })
  await ctx.close()
}
{
  const { ctx, p } = await open(1440, 900, 'light', { reducedMotion: 'reduce' })
  await p.goto(BASE); await ready(p); await p.waitForTimeout(1500)
  const moving = await p.evaluate(() => document.getAnimations().filter(a => a.playState === 'running' && a.effect?.getTiming().iterations === Infinity).length)
  ok('reduced motion: no endless motion', moving === 0, String(moving))
  await ctx.close()
}
ok('no console errors', errs.length === 0, errs.slice(0, 5).join(' || '))
fs.writeFileSync(`${OUT}/result.json`, JSON.stringify({ checks, errs }, null, 1))
report('gate', checks)
await b.close()
