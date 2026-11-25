# OpenRO v1.1.0 Studio Copilot Workbench Coverage (`data-world="studio"`)

OpenRO uses role-driven Desktop-Workbench primitives in `frontend/src/components/ui.tsx` powered by `motion/react` (Emil Kowalski spring physics + `AnimatePresence`), `vaul`, theme-synced `sonner`, and `lucide-react` 1.5px icons.

| Role | Implementation | Source | Notes |
|---|---|---|---|
| Action triggers | `Button` (monochrome ink + specular) | `src/components/ui.tsx` | `#111318` / `#F3F4F6` primary CTAs with `active:scale-[0.97]` (`150ms`), 44px mobile touch floor |
| Modal dialogs | `Dialog` + `MorphingDialog` + `AnimatePresence` | `src/components/ui.tsx` | Focus trap, `Escape` restore, `220ms` opacity + translate entrance (no `scale(0)`) |
| Segmented controls | `SegmentedControl` + `motion.span layoutId` | `src/components/ui.tsx` | Isolated `LayoutGroup` per control bar with `{ stiffness: 450, damping: 35, mass: 0.7 }` |
| Pane & drawer transitions | `AnimatePresence` + `motion.div` | `src/App.tsx`, `src/app/StudioBottomDrawer.tsx` | Collapsible Explorer rail, Workbench pane, and bottom drawer (`220ms`, `[0.23, 1, 0.32, 1]`) |
| Live-work cues | `TextShimmer`, `BorderTrail` | `src/components/ui.tsx`, `src/app/Thread.tsx` | Mounted strictly while actively building/playing; static under reduced motion |
| Mobile bottom sheet | `vaul` (`Drawer`) | `src/App.tsx`, `src/app/StudioBottomDrawer.tsx` | Native touch drawer on `<1100px` viewports |
| Status & notifications | `Pill`, `Badge`, `ProgressCircle`, `sonner` | `src/components/ui.tsx`, `src/App.tsx` | Theme-synced Light/Dark `Toaster` and `tabular-nums` counters |
| Inputs & shortcuts | `Input`, `Textarea`, `InputGroup`, `Kbd` | `src/components/ui.tsx` | `>= 3:1` `--field-border`, themed `caret-color`, 2px `:focus-visible` ring |
| Theme switch (#67) | Settings modal switch (`data-world="studio"`) | `src/app/SettingsDialog.tsx`, `src/theme.css` | Light/Dark full first-class themes; never on top bar or left rail |
