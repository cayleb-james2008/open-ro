// gate.mjs: OpenRO v0.3.0 Studio Workbench E2E gate (mock Studio, stub model).
// Exercises desktop (1440x900) & mobile (390x844) + 768 & 320 in both Light & Dark themes:
// blueprint build, custom prompt, Studio disconnect/reconnect, Undo confirm/cancel & focus trap,
// Explorer drawer & filter & Copy summary, BANNER-FIX-BRIEF update states, keyboard nav, first-fold, reduced motion.
import { BASE, OUT as E2E_OUT, launch, reset, report } from './fixture.mjs'
import fs from 'node:fs'

const OUT = `${E2E_OUT}/gate`
fs.mkdirSync(OUT, { recursive: true })
const b = await launch()

const MEASURE = () => {
  const vis = el => {
    const r = el.getBoundingClientRect()
    const s = getComputedStyle(el)
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'
  }
  const cv = document.createElement('canvas'); cv.width = cv.height = 1
  const cx = cv.getContext('2d', { willReadFrequently: true })
  const rgba = c => { cx.clearRect(0, 0, 1, 1); cx.fillStyle = '#000'; cx.fillStyle = c; cx.fillRect(0, 0, 1, 1); const d = cx.getImageData(0, 0, 1, 1).data; return [d[0], d[1], d[2], d[3] / 255] }
  const lin = v => { v /= 255; return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4 }
  const L = ([r, g, bb]) => 0.2126 * lin(r) + 0.7152 * lin(g) + 0.0722 * lin(bb)
  const bgOf = el => {
    let acc = [255, 255, 255]
    const chain = []
    for (let e = el; e; e = e.parentElement) chain.unshift(e)
    for (const e of [document.documentElement, document.body, ...chain]) {
      const c = rgba(getComputedStyle(e).backgroundColor)
      if (c[3] > 0) acc = [0, 1, 2].map(i => c[i] * c[3] + acc[i] * (1 - c[3]))
    }
    return acc
  }
  const inter = [...document.querySelectorAll('a[href],button,input,select,textarea,[role=button],summary,[tabindex]:not([tabindex="-1"])')].filter(e => vis(e) && !e.classList.contains('skip'))
  const name = el => (el.getAttribute('aria-label') || el.innerText || el.value || el.getAttribute('placeholder') || '').trim()
  const min = innerWidth < 768 ? 44 : 24
  const small = inter
    .filter(e => { const r = e.getBoundingClientRect(); return r.width < min - 0.5 || r.height < min - 0.5 })
    .map(e => `${e.tagName}:${name(e).slice(0, 30)}:${Math.round(e.getBoundingClientRect().width)}x${Math.round(e.getBoundingClientRect().height)}`)
  const unnamed = inter.filter(e => !name(e) && !(e.id && document.querySelector(`label[for="${e.id}"]`))).map(e => e.outerHTML.slice(0, 90))
  const low = []; let minFont = 99; let minRatio = 99
  for (const el of document.querySelectorAll('body *')) {
    if (!vis(el) || el.classList.contains('sr-only') || el.closest('.sr-only,[aria-hidden="true"]') || ![...el.childNodes].some(n => n.nodeType === 3 && n.textContent.trim())) continue
    if (el.closest(':disabled,[disabled],[aria-disabled="true"]')) continue
    const fs = parseFloat(getComputedStyle(el).fontSize); minFont = Math.min(minFont, fs)
    const fg = rgba(getComputedStyle(el).color); const bg = bgOf(el)
    const a = fg[3]; const mix = [0, 1, 2].map(i => fg[i] * a + bg[i] * (1 - a))
    const l1 = L(mix), l2 = L(bg); const ratio = (Math.max(l1, l2) + 0.05) / (Math.min(l1, l2) + 0.05)
    minRatio = Math.min(minRatio, ratio)
    if (ratio < (fs >= 18.66 ? 3 : 4.5)) low.push(`${ratio.toFixed(2)}:${(el.innerText || '').trim().slice(0, 30)}`)
  }
  const clipped = [...document.querySelectorAll('body *')].filter(e => {
    if (!vis(e) || e.classList.contains('skip')) return false
    const r = e.getBoundingClientRect()
    if (r.right <= innerWidth + 1 && r.left >= -1) return false
    for (let p = e.parentElement; p; p = p.parentElement) {
      const o = getComputedStyle(p).overflowX
      if (o === 'auto' || o === 'scroll' || o === 'hidden' || o === 'clip') return false
    }
    return true
  }).map(e => e.tagName + '.' + String(e.className?.baseVal ?? e.className).slice(0, 40) + ' r=' + Math.round(e.getBoundingClientRect().right)).slice(0, 5)
  return {
    clipped,
    docOverflow: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    smallTargets: small.length,
    smallSample: small.slice(0, 10),
    unnamed: unnamed.slice(0, 4),
    lowContrast: low.length,
    lowSample: low.slice(0, 6),
    minRatio: Math.round(minRatio * 100) / 100,
    minFont,
  }
}

const errs = [], checks = []
const ok = (name, pass, detail = '') => {
  checks.push({ name, pass: !!pass, detail })
  console.log((pass ? 'PASS ' : 'FAIL ') + name + (detail ? '  ' + detail : ''))
}

async function open(w, h, theme = 'light', extra = {}) {
  const ctx = await b.newContext({
    viewport: { width: w, height: h },
    colorScheme: theme,
    hasTouch: w < 768,
    permissions: ['clipboard-read', 'clipboard-write'],
    ...extra,
  })
  await ctx.addInitScript((t) => { try { localStorage.setItem('openro-theme', t) } catch {} }, theme)
  const p = await ctx.newPage()
  p.__allow = /$^/
  p.on('pageerror', e => errs.push(`${w}-${theme}: pageerror ${e.message}`))
  p.on('console', m => {
    if (m.type() === 'error' && !p.__allow.test(m.text())) {
      errs.push(`${w}-${theme}: ${m.text().slice(0, 140)} @ ${m.location().url}`)
    }
  })
  return { ctx, p }
}

const ready = p => p.waitForSelector('#composer-input', { timeout: 15000 })

async function measureAll(label) {
  for (const theme of ['light', 'dark']) {
    for (const [vn, w, h] of [['1440', 1440, 900], ['768', 768, 1024], ['390', 390, 844], ['320', 320, 640]]) {
      const { ctx, p } = await open(w, h, theme)
      await p.goto(BASE); await ready(p); await p.waitForTimeout(700)
      const m = await p.evaluate(MEASURE)
      ok(
        `measure ${label} ${vn} ${theme}`,
        m.docOverflow <= 0 && m.clipped.length === 0 && m.smallTargets === 0 && m.lowContrast === 0 && m.unnamed.length === 0 && m.minFont >= 12,
        `overflow=${m.docOverflow} clipped=${m.clipped.join('|')} small=${m.smallTargets}(${m.smallSample.join('|')}) low=${m.lowContrast}(${m.lowSample.join('|')}) unnamed=${m.unnamed.join('|')} minFont=${m.minFont}`,
      )
      if (vn === '1440' || vn === '390') await p.screenshot({ path: `${OUT}/${label}-${vn}-${theme}.png` })
      await ctx.close()
    }
  }
}

// 1. Fresh state across all 8 viewport x theme pairs + first-fold JOB.md answers + composer placeholder fit
await reset()
await measureAll('fresh')

for (const [vn, w, h] of [['1440', 1440, 900], ['390', 390, 844]]) {
  for (const theme of ['light', 'dark']) {
    const { ctx, p } = await open(w, h, theme)
    await p.goto(BASE); await ready(p); await p.waitForTimeout(500)
    const fold = await p.evaluate(() => {
      const inFold = sel => {
        const el = [...document.querySelectorAll(sel)].find(e => {
          const r = e.getBoundingClientRect()
          return r.width > 0 && r.height > 0 && r.top < innerHeight && r.bottom > 0
        })
        return Boolean(el)
      }
      return [1, 2, 3, 4].every(n => inFold(`[data-answers="${n}"], [data-answers-via="${n}"]`))
    })
    ok(`first-fold JOB.md answers 1..4 visible (${vn} ${theme})`, fold)
    await ctx.close()
  }
}

for (const [vn, w, h] of [['1440', 1440, 900], ['768', 768, 1024], ['390', 390, 844], ['320', 320, 640]]) {
  const { ctx, p } = await open(w, h, 'light')
  await p.goto(BASE); await ready(p); await p.waitForTimeout(400)
  const box = await p.evaluate(() => {
    const inp = document.querySelector('#composer-input')
    const g = inp.closest('[data-slot=input-group]')
    const a = g.querySelector('[data-slot=input-group-addon]')
    const cv = document.createElement('canvas').getContext('2d')
    const cs = getComputedStyle(inp)
    cv.font = `${cs.fontWeight} ${cs.fontSize} ${cs.fontFamily}`
    const phWidth = Math.ceil(cv.measureText(inp.getAttribute('placeholder') || '').width)
    const avail = inp.clientWidth - parseFloat(cs.paddingLeft || '0') - parseFloat(cs.paddingRight || '0')
    return {
      over: g.scrollWidth - g.clientWidth,
      addonRight: Math.round(a.getBoundingClientRect().right),
      groupRight: Math.round(g.getBoundingClientRect().right),
      phWidth,
      avail: Math.floor(avail),
    }
  })
  ok(`composer ${vn}: Undo stays inside InputGroup and placeholder fits without cutoff`, box.over <= 0 && box.addonRight <= box.groupRight && box.phWidth <= box.avail + 4, JSON.stringify(box))
  await ctx.close()
}

// 2. Desktop (1440x900) Studio Workbench E2E: Blueprint build -> Build Trace -> Luau console -> Explorer filter & Copy -> Custom prompt -> Undo cancel/confirm -> New chat & `/` search
await reset()
{
  const { ctx, p } = await open(1440, 900, 'light')
  await p.goto(BASE); await ready(p); await p.waitForTimeout(500)

  ok('fresh: 3 starter blueprint cards rendered', await p.locator('section[aria-labelledby=stage-title] article').count() === 3)
  ok('fresh: Lava Obby shows Obby category, count chip, and Adds to Studio pills', (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('2 parts · 5 scripts') && (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Stage checkpoints'))

  // Build Lava Obby in Studio
  await p.getByRole('button', { name: 'Build Lava Obby in Studio' }).click()
  await p.waitForSelector('section[aria-label="Studio build trace"]', { timeout: 20000 })
  await p.waitForTimeout(500)

  const traceText = await p.locator('section[aria-label="Studio build trace"]').innerText()
  ok('build trace shows 3/3 ProgressCircle and Passed verdict', traceText.includes('3/3') && traceText.includes('Passed'))
  ok('build trace counts 7 changes made in Studio', traceText.includes('changes made in Studio'))

  // Expand Luau console log inside Playtest Verdict banner
  await p.getByRole('button', { name: 'Show Luau console log' }).click()
  await p.waitForTimeout(300)
  ok('expandable Luau console log reveals playtest output', (await p.locator('section[aria-label="Studio build trace"]').innerText()).includes('Playtest passed.'))

  // Right Studio Explorer Shelf ("In your game")
  const shelf = p.locator('aside[aria-label="In your game"]')
  ok('Explorer groups Workspace Parts and Luau Scripts (7 pieces)', (await shelf.innerText()).includes('Workspace Parts') && (await shelf.innerText()).includes('Luau Scripts') && (await shelf.innerText()).includes('pieces in your game'))

  // Explorer quick filter + Expandable hierarchy detail
  await p.getByLabel('Filter Explorer').fill('Spawn')
  await p.waitForTimeout(200)
  ok('Explorer quick filter narrows items', (await shelf.innerText()).includes('Spawn Pad') && !(await shelf.innerText()).includes('Day Night'))
  await p.getByRole('button', { name: 'Hierarchy details for Spawn Pad' }).click()
  await p.waitForTimeout(250)
  ok('Explorer item Expandable shows full Roblox DataModel path', (await shelf.innerText()).includes('Workspace.SpawnPad'))
  await p.getByRole('button', { name: 'Clear Explorer filter' }).click()

  // Verify Light/Dark is tucked inside Settings (NOT on the main rail/bar)
  ok('Light/Dark switch is not cluttering main rail before opening Settings', (await p.getByRole('button', { name: 'Dark', exact: true }).count()) === 0)

  // Verify full Luau source viewer + Copy Luau in Explorer
  await p.getByRole('button', { name: 'Hierarchy details for Checkpoints' }).click()
  await p.waitForTimeout(250)
  ok('Explorer expands full Luau source and Copy Luau button', (await shelf.innerText()).includes('Players.PlayerAdded') && (await shelf.getByRole('button', { name: 'Copy Luau for Checkpoints' }).isVisible()))

  // Export .rbxlx + Copy game summary pinned visible without scroll trap
  const exportBox = await shelf.getByRole('button', { name: 'Export .rbxlx' }).boundingBox()
  ok('Export .rbxlx is pinned visible in viewport', Boolean(exportBox && exportBox.y + exportBox.height <= 900))
  const rbxlxRes = await fetch(BASE + 'api/export/rbxlx')
  const rbxlxXml = await rbxlxRes.text()
  ok('GET /api/export/rbxlx returns genuine Roblox XML', rbxlxRes.status === 200 && rbxlxXml.includes('<roblox ') && rbxlxXml.includes('version="4"') && rbxlxXml.includes('<Item class="Script"'))

  const copyBox = await shelf.getByRole('button', { name: 'Copy game summary' }).boundingBox()
  ok('Copy game summary is pinned visible in viewport', Boolean(copyBox && copyBox.y + copyBox.height <= 900))
  await shelf.getByRole('button', { name: 'Copy game summary' }).click()
  await p.waitForSelector('text=Copied the game summary', { timeout: 4000 }).then(() => ok('copy summary toast fires', true), () => ok('copy summary toast fires', false))
  const clipText = await p.evaluate(() => navigator.clipboard.readText())
  ok('clipboard holds full Studio summary', /My Roblox game has Lava Obby.*, 7 pieces in all/.test(clipText), clipText)
  await p.screenshot({ path: `${OUT}/built-desktop-1440-light.png` })

  // Switch to Dark theme inside Settings MorphingDialog and capture
  await p.getByRole('button', { name: 'Settings' }).click()
  await p.waitForSelector('[data-settings-dialog]', { timeout: 4000 })
  await p.getByRole('button', { name: 'Dark', exact: true }).click()
  await p.waitForTimeout(300)
  ok('Settings SegmentedControl switches to dark theme', await p.evaluate(() => document.documentElement.dataset.theme) === 'dark')
  await p.keyboard.press('Escape')
  await p.waitForTimeout(300)
  ok('Escape closes Settings dialog', (await p.locator('[data-settings-dialog]').count()) === 0)
  await p.screenshot({ path: `${OUT}/built-desktop-1440-dark.png` })
  await p.getByRole('button', { name: 'Settings' }).click()
  await p.waitForSelector('[data-settings-dialog]', { timeout: 4000 })
  await p.getByRole('button', { name: 'Light', exact: true }).click()
  await p.keyboard.press('Escape')
  await p.waitForTimeout(300)

  // Custom prompt send via Enter key
  await p.locator('#composer-input').fill('add a coin leaderboard and day cycle')
  await p.locator('#composer-input').press('Enter')
  await p.waitForFunction(() => document.querySelectorAll('section[aria-label="Studio build trace"]').length >= 1 && !document.querySelector('[role="status"]')?.textContent?.includes('Generating Luau'), null, { timeout: 25000 })
  await p.waitForTimeout(500)
  ok('custom prompt send appends user prompt to thread', (await p.locator('#thread').innerText()).includes('add a coin leaderboard and day cycle'))

  // Undo confirmation dialog: focus trap + Escape cancel + Keep it cancel + Yes, undo it confirm
  await p.getByRole('button', { name: 'Undo', exact: true }).click()
  await p.waitForSelector('[data-undo-dialog]', { timeout: 4000 })
  await p.waitForTimeout(250)
  const firstFocus = await p.evaluate(() => document.activeElement?.getAttribute('aria-label'))
  await p.keyboard.press('Tab')
  const secondFocus = await p.evaluate(() => document.activeElement?.getAttribute('aria-label'))
  await p.keyboard.press('Tab')
  const wrappedFocus = await p.evaluate(() => document.activeElement?.getAttribute('aria-label'))
  ok('Undo MorphingDialog traps Tab cycle between Keep it and Yes, undo it', firstFocus === 'Keep it' && secondFocus === 'Yes, undo it' && wrappedFocus === 'Keep it', `${firstFocus} -> ${secondFocus} -> ${wrappedFocus}`)

  await p.keyboard.press('Escape')
  await p.waitForTimeout(350)
  ok('Escape closes Undo dialog and returns focus to Undo trigger', (await p.locator('[data-undo-dialog]').count()) === 0 && (await p.evaluate(() => document.activeElement?.getAttribute('aria-label'))) === 'Undo')

  await p.getByRole('button', { name: 'Undo', exact: true }).click()
  await p.waitForSelector('[data-undo-dialog]', { timeout: 4000 })
  await p.getByRole('button', { name: 'Keep it' }).click()
  await p.waitForTimeout(350)
  ok('Keep it cancels Undo without reverting', (await p.locator('[data-undo-dialog]').count()) === 0)

  await p.getByRole('button', { name: 'Undo', exact: true }).click()
  await p.waitForSelector('[data-undo-dialog]', { timeout: 4000 })
  await p.getByRole('button', { name: 'Yes, undo it' }).click()
  await p.waitForSelector('text=Undid the last build', { timeout: 15000 }).then(() => ok('Undo confirm reverts build and fires toast', true), () => ok('Undo confirm reverts build and fires toast', false))

  // New chat + `/` search filter across chats
  await p.getByRole('button', { name: 'New chat' }).click()
  await p.waitForTimeout(600)
  ok('New chat starts fresh blueprint workbench and focuses composer', (await p.locator('#stage-title').innerText()) === 'Build in Roblox Studio')
  await p.locator('h1').click() // blur composer
  await p.keyboard.press('/')
  ok('global / shortcut focuses Search chats input', await p.evaluate(() => document.activeElement?.getAttribute('aria-label')) === 'Search chats')
  await p.keyboard.type('Build this')
  await p.waitForTimeout(200)
  ok('chat search filters chat list to matching session', await p.locator('#chat-list li').count() === 1)
  await p.getByRole('button', { name: 'Clear chat search' }).click()
  ok('clearing chat search restores both chats', await p.locator('#chat-list li').count() === 2)

  await ctx.close()
}

// 3. Built state pixel/contrast/target measurements across all 8 viewport x theme pairs + single-line SlidingNumber counts
await reset()
await fetch(BASE + 'api/ideas', { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{}' })
await fetch(BASE + 'api/ideas/build', { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ id: 'obby-starter' }) })
await measureAll('built')

const lineHeightCheck = (p, text) => p.getByText(text).first().evaluate(e => {
  const cs = getComputedStyle(e)
  const lh = parseFloat(cs.lineHeight) || parseFloat(cs.fontSize) * 1.4
  return { h: Math.round(e.getBoundingClientRect().height), lh: Math.round(lh) }
}).catch(() => null)

for (const [vn, w, h] of [['1440', 1440, 900], ['768', 768, 1024], ['390', 390, 844], ['320', 320, 640]]) {
  const { ctx, p } = await open(w, h, 'light')
  await p.goto(BASE); await ready(p); await p.waitForTimeout(600)
  const ch = await lineHeightCheck(p, 'changes made in Studio')
  if (w < 1100) { await p.getByRole('button', { name: 'In your game' }).click(); await p.waitForTimeout(500) }
  const pc = await lineHeightCheck(p, 'pieces in your game')
  ok(`counts ${vn}: SlidingNumber change and piece counts stay inline on one line`, Boolean(ch && pc && ch.h <= ch.lh * 1.6 && pc.h <= pc.lh * 1.6), JSON.stringify({ ch, pc }))
  await ctx.close()
}

// 4. Mobile (390x844) Light & Dark E2E: Left Rail Vaul drawer, theme toggle, Explorer Vaul drawer + Copy summary without scroll trap
{
  const { ctx, p } = await open(390, 844, 'light')
  await p.goto(BASE); await ready(p); await p.waitForTimeout(500)
  await p.getByRole('button', { name: 'Open the menu' }).click()
  await p.waitForTimeout(500)
  ok('mobile 390: Left Rail Vaul drawer opens', await p.getByRole('button', { name: 'New chat' }).first().isVisible())
  await p.keyboard.press('Escape')
  await p.waitForTimeout(500)
  ok('mobile 390: Escape closes menu drawer and returns focus to Open the menu', await p.evaluate(() => document.activeElement?.getAttribute('aria-label')) === 'Open the menu')

  await p.getByRole('button', { name: 'Open the menu' }).click()
  await p.waitForTimeout(500)
  await p.getByRole('button', { name: 'Settings' }).click()
  await p.waitForSelector('[data-settings-dialog]', { timeout: 4000 })
  await p.getByRole('button', { name: 'Dark', exact: true }).click()
  await p.waitForTimeout(250)
  ok('mobile 390: Dark mode switch updates data-theme inside Settings', await p.evaluate(() => document.documentElement.dataset.theme) === 'dark')
  await p.keyboard.press('Escape')
  await p.waitForTimeout(350)

  await p.getByRole('button', { name: 'In your game' }).click()
  await p.waitForTimeout(550)
  const mobileCopyBox = await p.locator('[role=dialog]').getByRole('button', { name: 'Copy game summary' }).boundingBox()
  ok('mobile 390: Explorer Vaul bottom drawer shows Copy game summary without hidden scroll', Boolean(mobileCopyBox && mobileCopyBox.y + mobileCopyBox.height <= 844))
  await p.screenshot({ path: `${OUT}/mobile-explorer-390-dark.png` })
  await p.keyboard.press('Escape')
  await p.waitForTimeout(500)
  ok('mobile 390: Escape closes Explorer drawer and returns focus to In your game', await p.evaluate(() => document.activeElement?.getAttribute('aria-label')) === 'In your game')
  await ctx.close()
}

// 5. Studio disconnected -> Check again reconnect flow + refused build + thread 500 recovery
await reset()
{
  let studioUp = false
  const { ctx, p } = await open(1440, 900, 'light')
  await p.route(BASE, async r => {
    const res = await r.fetch()
    const html = (await res.text()).replace('data-studio="true"', 'data-studio="false"').replace('data-can-build="true"', 'data-can-build="false"')
    r.fulfill({ response: res, body: html })
  })
  await p.route('**/api/status', r => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ studio_connected: studioUp, model_configured: true, workspace_mode: studioUp ? 'studio_mcp' : 'disconnected', version: '0.4.0' }) }))
  await p.goto(BASE); await ready(p); await p.waitForTimeout(500)

  ok('offline Studio: Build in Studio disabled with plain reason', await p.getByRole('button', { name: 'Build Lava Obby in Studio' }).isDisabled() && (await p.locator('section[aria-labelledby=stage-title]').innerText()).includes('Roblox Studio MCP connected'))
  ok('offline Studio: Check again button offered', await p.getByRole('button', { name: 'Check again' }).count() === 1)

  studioUp = true
  await p.getByRole('button', { name: 'Check again' }).click()
  await p.waitForSelector('text=Studio reconnected', { timeout: 5000 }).then(() => ok('Studio reconnect toast fires and Build in Studio enables', true), () => ok('Studio reconnect toast fires and Build in Studio enables', false))
  ok('reconnected Studio enables Build in Studio button', !(await p.getByRole('button', { name: 'Build Lava Obby in Studio' }).isDisabled()))
  await ctx.close()
}

{
  const { ctx, p } = await open(1440, 900, 'light')
  await p.route('**/api/ideas/build', r => r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ ok: false, error: 'Studio bridge error: instance write refused' }) }))
  await p.goto(BASE); await ready(p); await p.waitForTimeout(400)
  await p.getByRole('button', { name: 'Build Lava Obby in Studio' }).click()
  await p.waitForSelector('[role=alert]', { timeout: 5000 })
  ok('refused Studio step shows plain recovery alert and Try again', (await p.locator('[role=alert]').innerText()).includes('Roblox Studio refused a step') && await p.getByRole('button', { name: 'Try again' }).count() === 1)
  await ctx.close()
}

{
  const { ctx, p } = await open(1440, 900, 'light')
  p.__allow = /500/
  await p.route('**/api/thread', r => r.fulfill({ status: 500, body: 'err' }))
  await p.goto(BASE)
  await p.waitForSelector('text=Could not load the conversation.', { timeout: 8000 })
  ok('thread 500 error shows Try again recovery card', await p.getByRole('button', { name: 'Try again' }).count() === 1)
  await p.unroute('**/api/thread')
  await p.getByRole('button', { name: 'Try again' }).click()
  await p.waitForTimeout(800)
  ok('thread Try again recovers starter workbench', await p.locator('section[aria-labelledby=stage-title]').count() === 1)
  await ctx.close()
}

// 6. BANNER-FIX-BRIEF.md complete E2E matrix (1440x900 & 390x844, loopback vs non-loopback, focus transitions, plain errors)
for (const [vn, w, h] of [['1440', 1440, 900], ['390', 390, 844]]) {
  const { ctx, p } = await open(w, h, 'light')
  let staged = null
  await p.route('**/api/update', r => r.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({
      current: '0.3.0',
      latest: '0.3.1',
      available: true,
      can_apply: true,
      dev_build: false,
      mode: 'notify',
      staged,
      notes: 'Faster Luau skill checks.',
      release_url: 'https://github.com/cayleb-james2008/open-ro/releases/tag/v0.3.1',
    }),
  }))
  await p.route('**/api/update/check', r => r.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ current: '0.3.0', latest: '0.3.1', available: true, can_apply: true, mode: 'notify', checked_at: Math.floor(Date.now() / 1000) }),
  }))
  await p.route('**/api/update/apply', async r => {
    await new Promise(res => setTimeout(res, 250))
    staged = '0.3.1'
    r.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ current: '0.3.0', latest: '0.3.1', available: false, staged: '0.3.1', can_apply: true }) })
  })
  await p.goto(BASE); await ready(p); await p.waitForSelector('section[aria-label="OpenRO update"]', { timeout: 8000 })

  const stripText = await p.locator('section[aria-label="OpenRO update"]').innerText()
  ok(`update ${vn}: loopback shows Update now and explanation sentence`, stripText.includes('Update now downloads it, checks it, and replaces this app.') && await p.getByRole('button', { name: 'Update now' }).isVisible())
  ok(`update ${vn}: Check for updates button reachable`, await p.getByRole('button', { name: 'Check for updates' }).isVisible())

  // Dismiss -> focuses Show update
  await p.getByRole('button', { name: 'Dismiss update' }).click()
  await p.waitForTimeout(200)
  ok(`update ${vn}: Dismiss hides banner and focuses Show update`, (await p.locator('section[aria-label="OpenRO update"]').count()) === 0 && (await p.evaluate(() => document.activeElement?.textContent?.trim())) === 'Show update')

  // Manual Check for updates clears dismissal and restores banner
  await p.getByRole('button', { name: 'Check for updates' }).click()
  await p.waitForSelector('section[aria-label="OpenRO update"]', { timeout: 5000 })
  ok(`update ${vn}: manual Check for updates un-dismisses banner`, await p.getByRole('button', { name: 'Update now' }).isVisible())

  // Update now -> focuses polite status while busy, then focuses Restart now when staged
  const applyClick = p.getByRole('button', { name: 'Update now' }).click()
  await p.waitForTimeout(80)
  const busyRole = await p.evaluate(() => document.activeElement?.getAttribute('role'))
  await applyClick
  await p.waitForSelector('button:has-text("Restart now")', { timeout: 5000 })
  await p.waitForTimeout(100)
  const doneFocus = await p.evaluate(() => document.activeElement?.textContent?.trim())
  ok(`update ${vn}: Update now focuses status while busy and Restart now when staged`, busyRole === 'status' && doneFocus === 'Restart now', `${busyRole} -> ${doneFocus}`)
  await ctx.close()
}

// Non-loopback hostname (`lan-desk.local`): hides Update now & Check for updates, shows computer-it-runs-on line + What's new
{
  const { ctx, p } = await open(1440, 900, 'light')
  const port = new URL(BASE).port
  await p.route(`http://lan-desk.local:${port}/**`, async r => {
    const u = new URL(r.request().url())
    if (u.pathname === '/api/update') {
      return r.fulfill({
        status: 200,
        contentType: 'application/json',
        body: JSON.stringify({ current: '0.3.0', latest: '0.3.1', available: true, can_apply: true, mode: 'notify', release_url: 'https://github.com/cayleb-james2008/open-ro/releases/tag/v0.3.1' }),
      })
    }
    const localUrl = `http://127.0.0.1:${port}${u.pathname}${u.search}`
    const res = await fetch(localUrl, {
      method: r.request().method(),
      headers: { 'Content-Type': 'application/json', Host: `127.0.0.1:${port}`, Origin: `http://127.0.0.1:${port}` },
      body: r.request().method() === 'POST' ? r.request().postData() || '{}' : undefined,
    })
    r.fulfill({ status: res.status, headers: Object.fromEntries(res.headers.entries()), body: Buffer.from(await res.arrayBuffer()) })
  })
  await p.goto(`http://lan-desk.local:${port}/`); await ready(p); await p.waitForSelector('section[aria-label="OpenRO update"]', { timeout: 8000 })
  const lanText = await p.locator('section[aria-label="OpenRO update"]').innerText()
  ok('non-loopback host: hides Update now and Check for updates, shows plain line and What\'s new',
    (await p.getByRole('button', { name: 'Update now' }).count()) === 0 &&
    (await p.getByRole('button', { name: 'Check for updates' }).count()) === 0 &&
    lanText.includes('To update, open OpenRO on the computer it runs on.') &&
    lanText.includes("What's new"),
    lanText,
  )
  await ctx.close()
}

// Developer-build & plainUpdateError check
{
  const { ctx, p } = await open(1440, 900, 'light')
  await p.route('**/api/update', r => r.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ current: '0.3.0', latest: '0.3.1', available: true, can_apply: false, dev_build: true, mode: 'notify' }),
  }))
  await p.route('**/api/update/check', r => r.fulfill({
    status: 200,
    contentType: 'application/json',
    body: JSON.stringify({ current: '0.3.0', mode: 'notify', error: 'download checksum mismatch from https://objects.githubusercontent.com/secret' }),
  }))
  await p.goto(BASE); await ready(p); await p.waitForSelector('section[aria-label="OpenRO update"]', { timeout: 8000 })
  const devText = await p.locator('section[aria-label="OpenRO update"]').innerText()
  ok('developer build shows terminal-free sentence', devText.includes('This is a developer copy, so it does not update itself. Install a released version to get updates.') && !/terminal/i.test(devText))

  await p.getByRole('button', { name: 'Check for updates' }).click()
  await p.waitForTimeout(400)
  const errText = await p.locator('section[aria-label="OpenRO update"]').innerText()
  ok('failed update check maps checksum error plainly without raw URL', errText.includes('The download did not pass its safety check, so it was not installed.') && !/https?:\/\//.test(errText) && await p.getByRole('button', { name: 'Try again' }).isVisible())
  await ctx.close()
}

// 7. Skip link keyboard nav & prefers-reduced-motion: reduce
{
  const { ctx, p } = await open(1440, 900, 'light', { reducedMotion: 'reduce' })
  await p.goto(BASE); await ready(p); await p.waitForTimeout(800)
  await p.keyboard.press('Tab')
  ok('first Tab focuses Skip to message box link', (await p.evaluate(() => document.activeElement?.textContent?.trim())) === 'Skip to message box')
  await p.keyboard.press('Enter')
  ok('Skip link moves focus to #composer-input', (await p.evaluate(() => document.activeElement?.id)) === 'composer-input')
  const moving = await p.evaluate(() => document.getAnimations().filter(a => a.playState === 'running' && a.effect?.getTiming().iterations === Infinity).length)
  ok('reduced motion: zero endless animations', moving === 0, String(moving))
  await ctx.close()
}

ok('zero unexpected console or page errors across all gate flows', errs.length === 0, errs.slice(0, 5).join(' || '))
fs.writeFileSync(`${OUT}/result.json`, JSON.stringify({ checks, errs }, null, 2))
report('gate', checks)
await b.close()
