# Decisions

## 2026-11-03 — iteration 003

- The truncated `.bootstrap` archive stays in history and is not a source import. The working tree is the product.
- Approval payload stays in the main process. The renderer only sends an id. That is stronger than echoing the file body with a token.
- Official CLI model routes start when the user sends a message. They do not get a second approval dialog. File writes and shell commands do.
- Blender is detection only. Docs that claimed a task runner were corrected.
- `/brag` (latent-spaces/brag) is a launch-video skill. This iteration polished the repo page with real screenshots and did not render a Hyperframes video.
