# OpenRO front end: DESIGN (round 2)

## The job

You describe a Roblox game in plain words; OpenRO builds it in Roblox Studio, shows each step, runs a test before it says done, and can undo the last build. A beginner uses it without instructions. The first screen answers: 1 What can I make? 2 Is Roblox Studio connected? 3 What has it built? 4 Where do I type?

## Region map (1440 x 900 and 390 x 844)

- Rail, left (264 px; a left drawer on phones): logo, game name, the pear New chat key, chat list with a gliding selection, appearance switch, version. Familiar on purpose.
- Top bar: chat title and the Studio status chip with its text (always readable down to 320 px: on phones the chip drops to its own row under the title). When Studio is off, one action sits beside it: Check again.
- Update strip (between the top bar and the thread): check, result line, Update now / Restart now / What's new / Dismiss, Show update after a dismissal, plain errors with Details. Shown on every page; the apply and check controls only on this machine.
- Thread (centre, 760 px max): the greeting and the idea tiles (Q1), then the conversation; the build tray with its stamps (Q3).
- Dock (bottom): a torn-paper strip holding Next ideas, the undo reason, the input with Undo, and Send (Q4).
- Parts shelf, right (300 px; a bottom drawer below 1100 px, opened from the top bar): "In your game", stickers by kind, counts that roll, Details that open (Q3, whole game).

## Signature object

The idea tiles and the build tray, drawn as card stock: a 2 px lower rim in a darker paper tone, a soft paper shadow, a 2 px lift on hover (spring), and a 1 px seat on press. The tile's clamped pitch opens with More; its "What it adds" chips stay visible; its Build key is pinned to the bottom so equal-height tiles line up. The build tray's three stamps fill in as steps arrive, with a counter ring and the verdict stamp.

## Paper and keys

- Grain: one SVG fractal-noise per theme on the page background (about 6% ink speckle on light paper, 4% light speckle on night paper). Tokens: `--grain`.
- Card stock: `.card-stock` (surface, hairline, `--rim` and `--paper-shadow` tokens derived with color-mix).
- One torn edge: the dock strip, a static polygon (`.torn`), no gradient.
- Keys: a flat pear fill with a 3 px lower edge (`--key-edge`, pear pushed toward ink) and a lit top line (`--key-lit`). Hover darkens the fill one step; press seats the key 2 px and shrinks the edge to 1 px. No gradient, no Cult TextureButton on this screen.
- Focus ring: `--focus` (ink blue), not the pear, so it reads on cream.

## Library rows (the eight)

| Library | Need of this product | Part used | File |
|---|---|---|---|
| Motion Primitives | The chat selection glides; the undo confirm grows from its key | AnimatedBackground (chat list), MorphingDialog (undo confirm) | `rune/used/components/ui/{animated-background,morphing-dialog}.tsx` |
| COSS UI | A hairline input with an Undo addon; the appearance switch | InputGroup, InputGroupInput, InputGroupAddon, SegmentedControl | `rune/used/components/ui/{input-group,input,segmented-control}.tsx` |
| Cult UI | Disclosures: More on a tile, Details on a part and on an agent line | Expandable, ExpandableTrigger | `rune/used/components/ui/expandable.tsx` |
| Kibo UI | Sticker chips: what an idea adds, a part's kind, the Studio state | Pill, PillStatus, PillIndicator | `rune/used/components/kibo-ui/pill/index.tsx` |
| Tremor | The build's progress as a figure (2/3, 3/3) | ProgressCircle | `rune/used/components/ProgressCircle/ProgressCircle.tsx` |
| Vaul | Phone: the parts shelf rises from the bottom; chats and settings slide in from the left | Drawer | `node_modules/vaul` (no copied file) |
| Sonner | Undo results and Studio reconnected, outside the thread | Toaster, toast | `node_modules/sonner` (no copied file) |
| Motion | Idea tiles lift and seat (spring); stamps settle; the working dot pulses; reduced motion honoured | motion.div, MotionConfig reducedMotion="user", presets from `@/motion` | `node_modules/motion`, `rune/used/motion.ts` |

No need here (one allowed): Avatar (COSS UI and Kibo UI AvatarStack). The screen has no people on it.

Removed in round 2: Cult UI TextureButton and Motion Primitives Border Trail and Text Shimmer. Each drew a gradient, and the house page check pins no `linear-gradient` in the shipped page. The building tile carries a static pear ring; live work shows a pulsing pear dot.

## Failure and information (round 2)

The server marks failures in its replies and in its thread lines: a refused send or build comes back with `ok:false` and a 502/503 status, and the failure line starts with the server's own wording (`could not`, `did not`, `Studio not connected`, `Studio bridge error`, `no model key`, `failed`, `refused`). Thread lines carry no separate flag. The page therefore uses one rule (`FAILURE` in `app/data.ts`) matched to the server's own failure wording. The built-in opening line ("Building it now ... no model call involved") is information: it is a cream card. The old rule matched `no model` and turned it coral; that false positive is gone.

## Parity inventory (old page = `git show` of 3e749a0 assets/index.html)

K kept (same words, same behaviour) · C changed on purpose (reason) · N not done.

- Shell: brand and game name (K); New chat, focus to the box (K); chat list with current chat and "working" flag (K); switch chat with its failure notice (K); unsent text kept per chat in session storage (K; restored on switch); appearance Light/Dark (C: Light first, Light is the native default); version link (K).
- Update: check, result line (up to date / available / installed / not checked / off), Update now (only on this machine and with `can_apply`), Restart now with the one-minute return check, Dismiss per version, Show update, plain error mapping (`plainUpdateError`, six cases; raw text in Details), Try again, focus kept on the changed line, polite live region, developer-build and off states, 30-minute re-check (K).
- Skip link "Skip to message box" (K).
- Top bar: status chip with its text at every width (C: phones show the chip on its own row so the title stays); Check again when Studio is off (K, the one studio action; it reports "Studio reconnected").
- Server banners (no key, Studio not connected, recovered file, with Dismiss): K.
- Thread: welcome (two variants) (K); idea tiles with pitch, "What it adds" chips, Build this (C: chips visible, pitch clamps to 4 lines with More); "Already in your game" when the server says the idea is built (K, via `built_titles`, round 2: this is the parity gap); "Already in your game, or not on offer now" for an unbuilt idea no longer offered (K); the ask as an ink card and OpenRO's words as cream cards (K); failure cards coral with the plain sentence and Details (K); verdict bands Passed / Failed / Not proven yet with console output (K); working line (K, now a pulsing dot); thread error with Try again (K); per-chat notices (K); the thread scrolls to its latest line (K).
- Composer: input label, placeholder, id (K); Send disabled while busy (K); send failures keep the words in the box (K); the server's 403/413/415 words (K).
- Build: failure notice in its chat (K); focus back to the pressed idea's key, else the box (K, round 2; a tile that became "Already in your game" has no key, so focus goes to the box, as the old page did).
- Undo: enabled only when the chat has a change set and Studio answers; reason text above the dock (K, moved from beside the input); confirm with "Keep it" first, Tab stays inside, Escape cancels and returns focus to Undo (K, verified in browser at all four widths; the dialog part was edited, see LOCAL-EDITS); results as notices and toasts (K).
- In your game: Scripts and Parts with counts, kind stickers (sky for server scripts, mint for in-the-world parts), raw path behind Details (K); empty and error text (K); collapse toggle (K); bottom drawer on narrow screens (C: Vaul).
- Reduced motion: transitions and animations collapse to the end state (K, measured: computed duration 0.01 ms; MotionConfig reducedMotion="user").
- Not done: the old page's Studio polling (the old page had none; the status is the state at load plus a check on request, as before).

## Pinned hooks and test changes (cargo test: 17 lib + 88 integration pass)

Four pins were rewritten (commits marked `tests:`). Each one checks something the shipped page still guarantees:
- `id="sidebar"`, `id="chat-list"`, `id="thread"`, `id="composer"`, `id="status-chip"` in the served HTML: the React ids are in the bundle as `chat-list`, `` id:`thread` ``, `` id:`composer` `` and the shell carries `data-app="openro"` and `data-chip`. The served page never contained the `id="..."` form (React renders at run time).
- `data-theme="dark"` as the default: now `data-theme="light"` (the native theme is light; dark is a full second theme).
- `--faint: #8e8ea5` and `--faint: #62627a`: dropped. Old palette tokens. Contrast is measured in the browser probe (below), not pinned.
- `localStorage.setItem('openro-theme'`, `getItem('openro-theme')`, `[data-theme="light"]`: the bundle is minified (double quotes, unquoted attributes), so the same behaviour is pinned in its shipped spelling. The theme is saved by `openroSetTheme` in the shell's head script.
- Kept as they were: the no-key and Studio-not-connected banners, `:focus-visible`, `for="composer-input"` (the shell carries the labelled input outside the app root), the recovered-banner rules, the escaped game name, `chat-btn`, `Appearance`.

## Measurements (browser probe, light and dark, widths 1440 / 768 / 390 / 320)

- Horizontal overflow: 0 px at every width and theme (document scrollWidth).
- Interactive controls under 44 x 44 px (built state): 0 at every width and theme.
- Unnamed controls: 0.
- Smallest text: 12 px (floor).
- Contrast (text against its nearest opaque background, WCAG AA 4.5, 3.0 large): 0 failing text elements on the built state in all eight combinations. The disabled Send key is exempt by WCAG and is no longer below 4.5 in any case.
- Console and page errors: 0 in the eight combinations (the 502 on a phone build is the server's own refused reply, filtered from the probe's console line and read as a notice).
- Undo confirm, measured: focus on "Keep it" on open; Tab moves to "Yes, undo it"; Tab again returns to "Keep it" (trapped); Escape closes and focus returns to Undo; the dialog is gone.
- Reduced motion: transition duration computed 0.01 ms.

Hallmark render lint (`sloplint --scope app --render`, `assets/index.html`): 57 FAIL / 2 REFLEX / 7 WARN in round 1 terms; 44 FAIL / 2 REFLEX / 5 WARN now. Residual FAIL groups, not fixed in this round:
- 31 x rule 48 (colour literals): Tailwind's default palette and shadow colours emitted for the copied parts' classes (`--color-red-500`, `--tw-shadow` black, `bg-white/40`, dark stroke tints). Needs a theme reset that keeps this product's tokens; not attempted.
- 5 x rule 39a (disabled by opacity): the COSS input group's `has-disabled:opacity-64`. Needs local edits to the input group.
- 4 x rule 15 (a transition that also fades the focus ring: `transition-colors`, `transition-shadow` in chips and disclosures) and 2 x rule 10a (`transition-all`, width transitions) inside copied parts.
- 1 x rule 40: the input's edge contrast (1.46:1 on the input group's hairline). Needs the field drawn in the muted token.
- 1 x rule 20: the stamp check reads the bundle's first CSS rule; the `/*!` stamp is kept at the top of `src/index.css` but the minified bundle does not start with it.
- 2 REFLEX 7b: pure #fff and #000 in Tailwind's default palette (same source as rule 48).

## Local edits and the copied parts

`rune/used/LOCAL-EDITS.md` lists each edit (one so far: MorphingDialogClose passes its rest props and takes a caller's aria-label; the copied part named every close button "Close dialog" and dropped data attributes). `frontend/scripts/sync-used.mjs` skips the files listed there. The Border Trail and Text Shimmer parts and the Cult TextureButton are no longer imported, so the sync removed them.

## Defects and decisions for the lead

- The server's `/api/ideas` now returns `built_titles` (round 2 server change); the old page had no way to say an idea was already built.
- Failure lines carry no structured flag; the page classifies them by the server's own failure wording (`FAILURE`). A flag in `/api/thread` would be cleaner.
- Focus after a build lands on the box at most widths: the pressed tile becomes "Already in your game" and has no key, which is what the old page did.
- Version 0.2.0 is what Cargo.toml reports; the art file says 0.3.0.

## Not done

- The remaining lint FAIL groups above; the unslop review and impeccable/design-taste passes; sloplint's dark-theme judgments.
- A stranger's judgement on the look (the lead's review).
- data-answers attributes on the first fold (round 2 skipped them by instruction).
- A keyboard run of the phone drawers and the phone shelf (Vaul's own focus trap; checked by eye, not by a probe).
