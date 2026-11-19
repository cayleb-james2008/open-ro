# Local edits to copied parts

Each entry is a change made in this folder after the copy. `npm run sync-used` skips the files listed here, so the edit survives a sync.

| Part | What changed | Why |
|---|---|---|
| `components/ui/morphing-dialog.tsx` (MorphingDialogClose) | Rest props are spread onto the button, and `aria-label` may be set by the caller (default stays "Close dialog"). | The copied part dropped `data-*` and labels, so the undo confirm's "Keep it" was named "Close dialog" and "Yes, undo it" could not be found by the page. The fix is the smallest that lets the page name its own buttons. |
