# OpenRO v1.1.0 Desktop-Workbench Component Coverage

OpenRO replaces third-party multi-wrapper UI bloat with a lean, accessible set of native Desktop-Workbench primitives in `frontend/src/components/ui.tsx` and reduced-motion utilities in `frontend/src/motion.ts`.

| Role | Implementation | Source | Notes |
|---|---|---|---|
| Action triggers | `Button` | `src/components/ui.tsx` | Keyboard focus ring, 44px touch floor on mobile, explicit disabled contrast (no opacity fade) |
| Modal dialogs | `Dialog` + `MorphingDialog` | `src/components/ui.tsx` | Focus trap, `Escape` close, focus restoration to trigger |
| Segmented controls | `SegmentedControl` + `AnimatedBackground` | `src/components/ui.tsx` | `aria-pressed` / `aria-current` active surface indicator |
| Collapsible drawers | `Disclosure` + `Expandable` | `src/components/ui.tsx` | CSS height/visibility state with reduced-motion respect |
| Mobile bottom sheet | `vaul` (`Drawer`) | `src/app/StudioBottomDrawer.tsx` | Native touch drawer on narrow viewports |
| Status & notifications | `Pill`, `Badge`, `ProgressCircle`, `sonner` | `src/components/ui.tsx`, `sonner` | `tabular-nums` counters and crisp status tones |
| Inputs & shortcuts | `Input`, `Textarea`, `InputGroup`, `Kbd` | `src/components/ui.tsx` | High-contrast hairline borders, accessible labels |
| Iconography | `lucide-react` | `lucide-react` | Uniform 1.5px stroke icons across chrome and workbench |
| Theme switch (#67) | Settings modal switch | `src/app/SettingsDialog.tsx` | Light/Dark full first-class tokens in `src/theme.css`; never on top bar or left rail |
