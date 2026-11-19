# rune/used: hand edits to copied parts

`frontend/scripts/sync-used.mjs` keeps every file listed here instead of copying the starter over it. Each edit is the smallest change that clears a Hallmark FAIL or WARN (round 3). Paths:

- `components/ProgressCircle/ProgressCircle.tsx`
- `components/kibo-ui/pill/index.tsx`
- `components/ui/badge.tsx`
- `components/ui/button.tsx`
- `components/ui/expandable.tsx`
- `components/ui/input-group.tsx`
- `components/ui/input.tsx`
- `components/ui/morphing-dialog.tsx`
- `components/ui/segmented-control.tsx`
- `components/ui/textarea.tsx`

| Part | What changed | Why |
| --- | --- | --- |
| `components/ui/input.tsx`, `input-group.tsx`, `textarea.tsx` | `transition-shadow` to colour, background and border colour; `has-disabled:opacity-64` to fill, ink and cursor (`text-disabled`); the `has-focus-visible` border ring to a 2px outline in `--focus` | the ring fades in (15); disabled by opacity alone (39a); focus by border (39a) |
| `components/ui/button.tsx`, `badge.tsx` | `transition-shadow` to colour; `dark:bg-input/32` and `/48` to `dark:bg-transparent`; `disabled:opacity-64` to ink | the ring fades in (15); the fill took the field edge colour and cut the text to 2.7:1 (contrast) |
| `components/ui/segmented-control.tsx` | `transition-[width,height,translate]` on the indicator to `transition-transform`; `dark:bg-input` to `dark:bg-raised`; disabled by opacity to ink | animated width and height (10a); the field edge is a 3:1 token now; disabled by opacity (39a) |
| `components/ui/expandable.tsx` | `transition-all` to `transition-[opacity,transform]`; the unused demo wrapper gets `bg-surface text-ink` in place of `bg-white` and a dark fill without a text colour | transition: all (10a); WARN 41 |
| `components/ui/morphing-dialog.tsx` | (round 2) rest props spread onto the close button and a caller's `aria-label` kept; scrim `bg-white/40` to `bg-ink/25`, dark `bg-black/40` to `bg-ink/40` | the copied part named both undo buttons "Close dialog" (round 2); colour literals (48) |
| `components/ProgressCircle/ProgressCircle.tsx` | `transition-colors` to `transition-[color,background-color]`; the stroke `transition-[stroke-dashoffset]` kept (a property, not a fade) | a ring fade on focus (15) |
| `components/kibo-ui/pill/index.tsx` | the mask gradient's `white` stop to `currentcolor` (alpha only) | a literal #fff in a mask (48) |
