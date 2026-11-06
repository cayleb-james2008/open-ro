# The house style — the shared foundation

Owner order, 4 Nov 2026. This is the contract the later surfaces consume: the **DSH face**, the **Ergon** surface
and the **landing page**. Everything here is our own code on shadcn primitives (a `cva` variant map, a `cn` joiner,
a `...rest` passthrough). The Square UI demos were design reference only; no free template was a source, and no
line of either was copied.

## The rules, in one place

| rule | what it means here |
|---|---|
| **Geist Sans + Geist Mono** | The UI word is set in Geist Sans; every number, id and timestamp is set in Geist Mono with tabular figures, so a column of figures lines up and the digits do not jitter when a value changes. |
| **A tight small-type scale** | 12 (a chip) / 13 (labels, ages, units, state words) / 14 (a secondary figure) / 24 (a stat on a shelf surface or on a phone) / 30 (a stat on a full surface). **Nothing above 14 px except a stat value.** |
| **Colour is data only** | A colour may sit on a status pill, a delta, a chart series, a progress fill, a guard tile or a state word. Everything else - the chrome, the surfaces, the type ramp - is a zero-chroma neutral. |
| **A 10 px radius base** | Cards, panels, sections, chips and buttons wear 10 px; a small control inside a card wears 8 px. Nothing is square-cornered. |
| **Hairlines do all the separation** | A 1 px border separates a card from the page, a row from a row. A card carries no shadow and no glow. |
| **Shadows only on floating layers** | `elevation.float` (in `tokens/house.css`) is for a popover, a dialog, a menu or a dragged card - the things that genuinely float. A surface never uses it. |
| **A dense 4 px grid** | Every space is a multiple of 4. The grid gap is 8 px, the card padding 12 px, the gutter 16 px. |
| **Ops surfaces default DARK** | The dark palette is the default; there is no light theme in v1. |

## How to consume it

**The tokens** come from one source, `tokens/tokens.json`, and are emitted three ways:

| file | what it is | who uses it |
|---|---|---|
| `src/tokens.css` | the custom properties this pack compiles in | this pack |
| `tokens/tailwind-theme.json` | this pack's Tailwind theme (colours, sizes, weights, spacing, radius, motion) | this pack |
| `tokens/house.css` | the portable custom properties, including the elevation token | any other surface: copy the file and use the properties |

```html
<!-- a surface that is not this pack -->
<link rel="stylesheet" href="house.css">
<style>
  .card { background: var(--c-panel); border: 1px solid var(--c-line); border-radius: var(--radius-base); }
  .stat { font-family: var(--font-mono); font-variant-numeric: tabular-nums; font-size: var(--fs-stat); font-weight: var(--fw-label); }
</style>
```

**The primitives** live in `src/ui/` and are ours. They are plain React with no runtime beyond `react`,
`class-variance-authority` and `clsx`:

| primitive | what it is | the house rule it carries |
|---|---|---|
| `Button` | `variant: primary \| secondary \| ghost`, `size: sm \| md`, `radius: base \| control` | the primary is the inverse of the surface it sits on: white on dark, black on light |
| `Card` | the surface: panel colour, one hairline, the 10 px radius; `tone: panel \| base \| raised`, `as` | a card never carries a shadow |
| `Pill` | `tone: live \| live-chain \| shadow \| manual \| synthetic \| crit \| warn \| info \| neutral` | the one place a colour may sit, and only when it carries a state |
| `Stat` | `size: stat \| stat-compact \| meta`, mono and tabular | the stat value is the only big type |
| `AnimatedNumber` | a value that changes, tweened from the reading on screen to the new one | never on load, never outside the two readings, never extrapolated; the age never moves |
| `TransitionPanel` | a panel that opens and closes, measured, not guessed | the content is never unmounted |
| `TextMorph` | a word that changes, lifted 2 px and settled | the DOM always holds a whole word; a transform, never a fade |
| `MorphingDialog` | a dialog that grows out of its trigger | the one place `elevation.float` belongs |

## The motion

Three motions, and no others: a **value that changed** tweens (280 ms), a **panel** opens (220 ms), a **word**
morphs (180 ms). They use the Motion Primitives patterns (MIT); the code is ours.

The honesty rules that keep the number tween honest, and that any surface adopting `AnimatedNumber` inherits:

- it never runs on the first paint - there is no count-up;
- it only moves between two served values, and lands exactly on the new one;
- it never extrapolates, never loops, never runs on its own;
- the age cell beside it never moves, and while a value is in flight the element carries `data-tween="1"` with
  `data-value` holding the **target** (the reading), so a checker reads the reading and not the frame.

Under `prefers-reduced-motion: reduce` all three are instant. The primitives ask the browser themselves
(`src/ui/useReducedMotion.js`), so a surface that adopts them cannot forget: an explicit `reducedMotion` prop still
wins, but the default is the reader's preference, and the preference is followed while the page is open.

## The fonts

Geist and Geist Mono are installed locally (`~/.local/share/fonts/openbell-house`, SIL OFL 1.1, licence texts
beside the files) and the page declares them by name, with a system fallback behind them. The page carries **no
font bytes**: no `@font-face`, no data URI, no extra request and no CSP change. A machine without the fonts
renders the fallback - which is why the licence texts sit next to the files and why the stack names both `Geist`
and `Geist Sans`.

If a surface must ship the fonts itself, it needs `font-src data:` in its CSP and a byte budget for them; that is
a decision for the surface, not for this token file.

## What is frozen everywhere

Shadow is never money; a shadow, paper, arena or fixture figure carries `SHADOW` or `SYNTHETIC` in its own row.
Every figure carries its age. The page is read-only and GET-only. A 3D view that cannot be drawn degrades to the
DOM twin with every fact intact. Those rules are not a style choice and no surface trades them away.
