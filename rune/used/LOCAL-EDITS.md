# rune/used: hand edits to copied parts

`frontend/scripts/sync-used.mjs` keeps every file listed here instead of copying the starter over it. Each edit is the smallest change that clears a Hallmark FAIL or accessibility/focus defect. Paths:

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
| `components/ui/button.tsx`, `badge.tsx` | `transition-shadow` to colour; `dark:bg-input/32` and `/48` to `dark:bg-transparent`; `disabled:opacity-64` to `disabled:border-hairline disabled:bg-track disabled:text-ink-3`; `ref` forwarded to `useRender` | the ring fades in (15); dark contrast; disabled contrast >=4.5:1 (40/41); Radix `Drawer.Trigger asChild` + caller `ref` focus restoration |
| `components/ui/segmented-control.tsx` | `transition-[width,height,translate]` on the indicator to `transition-transform duration-(--dur-swap) ease-(--ease-house)`; `bg-raised` on active indicator and checked items; disabled by opacity to ink | animated width and height (10a); active pill contrast in dark mode; disabled by opacity (39a) |
| `components/ui/expandable.tsx` | `transition-all` to `transition-[opacity,transform]`; `ExpandableTrigger` sets `aria-expanded={isExpanded}` from context; unused demo wrapper gets `bg-surface text-ink` | transition: all (10a); disclosure accessibility without hand-wiring in screens; WARN 41 |
| `components/ui/morphing-dialog.tsx` | rest props spread onto close button and caller's `aria-label` kept; `MorphingDialogTrigger` wires `ctxTriggerRef`, `type="button"`, `disabled={blocked}` and `aria-label`; unmount cleanup removes `overflow-hidden` and returns focus to trigger; flat `bg-ink/30` scrim without backdrop blur | Undo confirm button labels, disabled trigger state, automatic Escape/Keep-it focus return, and no-blur house rule |
| `components/ProgressCircle/ProgressCircle.tsx` | `transition-colors` to `transition-[color,background-color]`; the stroke `transition-[stroke-dashoffset]` kept (a property, not a fade) | a ring fade on focus (15) |
| `components/kibo-ui/pill/index.tsx` | the mask gradient's `white` stop to `currentcolor` (alpha only) | a literal #fff in a mask (48) |
