# rune/used: the library parts this app uses

Copied unchanged (apart from the house edits recorded in the starter's STACK.lock.md) from the owner's house-frontend starter by `frontend/scripts/sync-used.mjs`. Only parts the screens import are here; `npm run sync-used` refreshes this folder and this table. npm packages the parts need (react, motion, radix, vaul, sonner, recharts, lucide, ...) are in `frontend/package.json`.

| Part | Library | Licence | Imported by |
| --- | --- | --- | --- |
| `components/ProgressCircle/ProgressCircle.tsx` | Tremor Raw (tremor.so) | Apache-2.0 | src/app/Thread.tsx |
| `components/kibo-ui/pill/index.tsx` | Kibo UI (kibo-ui.com) | MIT | src/App.tsx, src/app/Blueprints.tsx, src/app/SettingsDialog.tsx, src/app/Shelf.tsx, src/app/StagePreview.tsx, src/app/Thread.tsx |
| `components/ui/animated-background.tsx` | Motion Primitives (motion-primitives.com) | MIT | src/App.tsx |
| `components/ui/avatar.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/kibo-ui/pill/index.tsx |
| `components/ui/badge.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/kibo-ui/pill/index.tsx, src/app/Blueprints.tsx |
| `components/ui/button.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/kibo-ui/pill/index.tsx, src/App.tsx, src/app/Blueprints.tsx, src/app/SettingsDialog.tsx, src/app/Shelf.tsx, src/app/StagePreview.tsx, src/app/Thread.tsx, src/app/Update.tsx |
| `components/ui/expandable.tsx` | Cult UI (cult-ui.com) | MIT | src/app/Shelf.tsx, src/app/StagePreview.tsx, src/app/Thread.tsx, src/app/Update.tsx |
| `components/ui/input-group.tsx` | COSS UI (coss.com/ui) | MIT | src/App.tsx, src/app/Shelf.tsx |
| `components/ui/input.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/ui/input-group.tsx, src/app/SettingsDialog.tsx |
| `components/ui/kbd.tsx` | COSS UI (coss.com/ui) | MIT | src/App.tsx |
| `components/ui/morphing-dialog.tsx` | Motion Primitives (motion-primitives.com) | MIT | src/App.tsx, src/app/SettingsDialog.tsx |
| `components/ui/segmented-control.tsx` | COSS UI (coss.com/ui) | MIT | src/app/SettingsDialog.tsx |
| `components/ui/sliding-number.tsx` | Motion Primitives (motion-primitives.com) | MIT | src/App.tsx, src/app/Shelf.tsx, src/app/Thread.tsx |
| `components/ui/spinner.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/ui/button.tsx, src/app/Thread.tsx |
| `components/ui/textarea.tsx` | COSS UI (coss.com/ui) | MIT | rune/used/components/ui/input-group.tsx |
| `hooks/useClickOutside.tsx` | House presets (house-frontend starter) | owner's | rune/used/components/ui/morphing-dialog.tsx |
| `lib/utils.ts` | House presets (house-frontend starter) | owner's | rune/used/components/kibo-ui/pill/index.tsx, rune/used/components/ui/animated-background.tsx, rune/used/components/ui/avatar.tsx, rune/used/components/ui/badge.tsx, rune/used/components/ui/button.tsx, rune/used/components/ui/expandable.tsx, rune/used/components/ui/input-group.tsx, rune/used/components/ui/input.tsx, rune/used/components/ui/kbd.tsx, rune/used/components/ui/morphing-dialog.tsx, rune/used/components/ui/segmented-control.tsx, rune/used/components/ui/spinner.tsx, rune/used/components/ui/textarea.tsx |
| `motion.ts` | House presets (house-frontend starter) | owner's | src/App.tsx, src/app/Blueprints.tsx, src/app/SettingsDialog.tsx, src/app/Thread.tsx |
| `utils/cx.ts` | House frontend starter | owner's | rune/used/components/ProgressCircle/ProgressCircle.tsx |
