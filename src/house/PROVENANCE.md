# Provenance — the real FRONTEND-LIBRARY standard files carried into Open-Ro

Mission m-0051 (Slack TICKET 008: Open-Ro Studio front-end rebuild). The ticket requires the fleet's actual
Square UI / Motion Primitives / Kitchen sources **as they exist on this machine**, from the Ergon and dashboard
builds — not re-sourced from the internet, not imitations. This directory is that set.

## Where the files came from (on this machine, verified byte-for-byte)

| file in this repo | source path on this machine | sha256 |
|---|---|---|
| `src/house/ui/AnimatedNumber.js` | `/home/cayleb/workspace/openbell-watchover/design/watchover/src/ui/AnimatedNumber.js` | `e8b2f0be4ed9b6ed7ea52ccafb79960c0ec9e95ca3d8619244fcc8c825f83b0c` |
| `src/house/ui/Button.js` | `…/watchover/src/ui/Button.js` | `407a78af9e2ac91be6d244d03206a6e02aefc4f72e24785c6e83961c9bc21b26` |
| `src/house/ui/Card.js` | `…/watchover/src/ui/Card.js` | `714a14a7b26c20d0c91b14b45a8e273cff194c75dc5ac0eda1b6becb9df8b93b` |
| `src/house/ui/Pill.js` | `…/watchover/src/ui/Pill.js` | `f960a803d7d93331e921d76c4bfa84d7129b8563177de9aad11747f02d9a1358` |
| `src/house/ui/Stat.js` | `…/watchover/src/ui/Stat.js` | `3f17ea73fee0255d99f2e6eae6da81835fcd5de56424e7216349aef51e816d86` |
| `src/house/ui/TextMorph.js` | `…/watchover/src/ui/TextMorph.js` | `7d8bc524d1e52bebff3e4d45cbbaf76ce453d733f44d79b493dc1990f56f46f8` |
| `src/house/ui/TransitionPanel.js` | `…/watchover/src/ui/TransitionPanel.js` | `2f7c7b22b286f1405fba7d5b8bc242a3ef5149b008f48689794d198c3f1a7f2c` |
| `src/house/ui/MorphingDialog.js` | `…/watchover/src/ui/MorphingDialog.js` | `6adf7af1f921f0c2a1a33af27691ae2967436d5596fcdef95aa2a4c0c1810520` |
| `src/house/ui/cn.js` | `…/watchover/src/ui/cn.js` | `9f341d275d98630dbcfa52a63eef033ebf73ef6d25feedf7d97de05f0a31e9e0` |
| `src/house/ui/ease.js` | `…/watchover/src/ui/ease.js` | `7db882c56b6997f96fa5ede61d7c47763781af357683898ae86102821017644c` |
| `src/house/ui/useReducedMotion.js` | `…/watchover/src/ui/useReducedMotion.js` | `187010655a15f4070ec3993dd24f351453e2eaf5b4a9fb689439ccf207ec336d` |
| `src/house/ui/index.js` | `…/watchover/src/ui/index.js` | `91bdc52afc2d0411655448d3933a0272074d2bdb138814fbbc27e77f250aca00` |
| `public/house/house.css` | `…/watchover/tokens/house.css` | `3dbd6e9538d3bf7791338761d44d64b6acd99e14b00af1b16d807273d438aee3` |
| `src/house/HOUSE-STYLE.md` | `…/watchover/tokens/HOUSE-STYLE.md` | `4aa1876620a8a4148a8de0b2ab42dbe28e6cbfc25fe95c53872b3e40cc81033c` |
| `src/house/tokens/tokens.json` | `…/watchover/tokens/tokens.json` | `d4ada101f2bb773e92d277951721cdd0d44cdc916ffad7710c366dc6fbb99f36` |
| `src/house/tokens/tailwind-theme.json` | `…/watchover/tokens/tailwind-theme.json` | `667329e6e0596ca4102e0fa5787403a28fe6a78fa0505a426219da819347dca6` |

`tailwind.config.cjs` and `postcss.config.cjs` are the pack's own files carried to the repo root, with only the
`content` glob widened to this app's extensions (`…/watchover/tailwind.config.cjs`, `…/watchover/postcss.config.cjs`).
`src/house/tokens/geist-*.txt` are the SIL OFL 1.1 licence texts that sit beside the Geist font files on this
machine (`/home/cayleb/.local/share/fonts/openbell-house/`).

## Kitchen (Tactile Button / Ticket)

The on-machine Kitchen source is the fleet dashboard build at
`/home/cayleb/.dsh-fleet-state/missions/m-0035/work/build-dashboard.mjs` (ticket 001, the accepted dashboard
build): `drawTicketNode()` (body, perforation, punched notches, stub) and the tactile-face CSS recipe the build
documents at its header (shaped face, firm edge, compressible depth). `src/house/kitchen/` carries the React
implementations of those same recipes, in the house style, with the lineage named in each file's header.

## Kitchen React carry

The same m-0035 build is the source for the React components in `src/house/kitchen/` (sha256
`82223b141867c2ce6df0c40561a2b2c81b0ddb1f253a9b978d8d5073af96547a`). `TactileButton.tsx` carries
`drawPanelNode()`'s tactile face — the `n-depth` wall behind the `n-face` plate, its `n-lip` top light
and the press down into the wall — as `.tactile-edge`, `.tactile-face` and the `:active` press in
`kitchen.css`. `Ticket.tsx` carries `drawTicketNode()` — the `n-face`/`n-depth` body, the dashed
`n-perf` tear line, the two `n-notch` punched circles and the stub's `n-meta` id line — as
`.ticket-body`, `.ticket-perf`, `.ticket-notch` and `.ticket-stub`. Both components name that lineage
in their own file headers; `kitchen.css` owns the component CSS and reads every colour from
`public/house/house.css`.

## Runtime deps added for the carried source

The design pack's own pinned deps, added to package.json: `clsx@2.1.1`, `class-variance-authority@0.7.1`
(runtime) and `tailwindcss@3.4.19` (dev, the pack's pinned compiler). No other dependency was added.
