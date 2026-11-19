# OpenRO front end: DESIGN (round 1 draft)

Status: round 1 of 2. Chat screen, shell, parts shelf and composer are built and pictured; the update banner, the phone menu's full behaviour and the rest of the parity list are not done (see "Parity inventory").

## The job, in three lines

You describe a Roblox game in plain words and OpenRO builds it in Roblox Studio, shows each step, runs a test before it says done, and can undo the last build. A beginner uses it with no instructions: it speaks first, shows what it built, and every change can be taken back. The screen answers four questions in the first screenful: 1 What can I make? 2 Is Roblox Studio connected? 3 What has it built so far? 4 Where do I type?

## Region map (1440 by 900 and 390 by 844)

- **Rail, left, 264 px (phone: a left drawer behind the menu key):** logo tile, game name, the pear "New chat" key, chat list with a gliding selection, appearance switch, version. Why: a chat list is the primary navigation between a game's build sessions; familiar on purpose (answers nothing itself, keeps the shell out of the way).
- **Top bar:** the chat title and the Studio status chip (live green dot only when connected). Answers Q2 in one glance. On a phone the chip keeps its dot and its words move to a screen-reader label, so the title stays readable.
- **Thread, centre, max 760 px:** the greeting (Q1) and the three idea tiles, then the conversation: the ask as an ink card, OpenRO's words as cream cards, and the build tray (Q3) inline. Why: the ideas are the thing the app says first, drawn big and real, not a list.
- **Composer dock, bottom of the centre:** "Next ideas" chips above the input row (input, Undo with its reason, Send). Answers Q4. Why: the place to type is always where a thumb or eye lands, and the one primary key is Send.
- **Parts shelf, right, 300 px (below 1100 px a bottom drawer opened from the top bar):** "In your game" on kraft-tinted board, scripts and parts as stickers with counts. Answers Q3 for the whole game, not just the last build. Collapsible.

## Signature object

**The idea tiles and the build tray.** Each idea is a large card-stock tile with a chunky title, one plain sentence, "What it adds" as a disclosure of sticker chips, and the pear "Build this" key. While an idea builds, its tile carries a BorderTrail. The build becomes a tray: three step stamps (Saved script, Generated, Tested) fill in as the steps arrive, a counter ring reads 2/3 or 3/3, and the verdict is the last stamp: a mint "Passed", a coral "Did not pass", or "Not proven yet".

## Library rows (the eight)

| Library | Need of this product | Part used | File |
|---|---|---|---|
| Motion Primitives | The selected chat glides in the rail; a working line shows that a build runs; the build tile shows it runs; the undo confirm grows out of its key | AnimatedBackground (chat list), TextShimmer (working lines), BorderTrail (building tile and tray), MorphingDialog (undo confirm) | `rune/used/components/ui/{animated-background,text-shimmer,border-trail,morphing-dialog}.tsx` |
| COSS UI | Compact hairline input with an addon for Undo; an icon switch for the appearance | InputGroup, InputGroupInput, InputGroupAddon, SegmentedControl, SegmentedControlItem | `rune/used/components/ui/{input-group,input,textarea,segmented-control}.tsx` |
| Cult UI | The one primary key of each screen (New chat, Build this, Send) with its slab; a disclosure for "What it adds" and for a part's Details | TextureButton (wrapped as `app/Key.tsx`, re-coloured to the pear token), Expandable, ExpandableTrigger, ExpandableContent | `rune/used/components/ui/{texture-button,expandable}.tsx` |
| Kibo UI | Sticker chips for the skills an idea adds, the parts on the shelf, and the Studio state | Pill, PillStatus, PillIndicator | `rune/used/components/kibo-ui/pill/index.tsx` |
| Tremor | A figure the person can read at a glance: how far the build has got | ProgressCircle (with the count inside) | `rune/used/components/ProgressCircle/ProgressCircle.tsx` |
| Vaul | Phone: the parts shelf rises from the bottom; the chats and settings slide in from the left | Drawer (Root, Portal, Overlay, Content, Handle, Title; direction left and bottom) | `node_modules/vaul` (the package; no copied file) |
| Sonner | Undo results outside the thread: "Undid the last build", failures | Toaster, toast.success, toast.error | `node_modules/sonner` (the package) |
| Motion | A press on an idea tile; the stamps and the verdict settle with a spring; new lines arrive once | motion.div (`whileTap`, `layout`, `initial`) with presets from `@/motion` (press, layout, reveal, open) | `rune/used/motion.ts`, `node_modules/motion` |

**No need here (one of three allowed):** COSS UI Avatar and Kibo UI AvatarStack. The screen has no people in it; an avatar would be decoration. Reviewer may dispute this.

Parts used in round 1 are also in `rune/used/MANIFEST.md` (21 parts, written by `npm run sync-used`).

## Parity inventory (old page = `git show HEAD:assets/index.html`)

Status key: K = kept with the same words and behaviour; C = changed on purpose (reason given); P = partly done; N = not done in round 1.

Shell and rail
- Brand, game name from the server (`__GAME_NAME__`): K.
- New chat: K. Creates a chat, selects it, refreshes everything, focuses the box. Failure toast "Could not reach the OpenRO server.": K.
- Chat list (titles, current chat, "working" flag on a busy chat): K. The old page's `aria-current` is kept on the chat button.
- Switch chat (select request, failure "Could not switch chats. The server did not answer."): K for the request; the failure notice is now a thread notice in the chat it came from.
- Unsent text per chat (draft saved on switch, restored on return): P. Saved and restored on switch; not yet restored on reload.
- Appearance: Dark/Light segmented control: C. Light is now listed first and is the default (art file: native theme LIGHT). Choice stored in `localStorage` `openro-theme`: K.
- Version link (`OpenRO <version>`, links to releases): K as a pill link.
- "Check for updates", result line, update banner (Update now / Restart now / Dismiss / Show update / What's new / plainUpdateError / dev-build and off states), 30-minute check: N. Round 2.
- Phone menu sheet (modal: focus moves in, Tab stays inside, Escape or scrim closes, focus returns to the menu key): P. Vaul drawer from the left; Vaul owns the focus trap and Escape. Not yet verified with a keyboard run.
- Skip link "Skip to message box": N. Dropped in this draft; restore in round 2.

Top bar and banners
- Status chip "Roblox Studio connected" / "Studio not connected" with live dot: K (rendered from the shell's server values).
- Server banners (no model key, Studio not connected, recovered file with Dismiss): K. Served as the server's own markup; Dismiss hides its banner.
- The old page has no timer for Studio: the chip is the state at load plus a refresh after each action. Kept as it was. (The brief mentions "Studio connection polling"; the old page had none, so none was added.)

Thread
- Welcome (two sentences, two variants by can-build): K.
- Idea tiles (title, pitch, "What it adds" chips, Build this, disabled when no Studio or busy, the "Connect Studio to build this" line): K. Changed: pitches and chips now live in a disclosure.
- Idea line whose record is not in the current list ("Already in your game, or not on offer now..."): N.
- The ask (ink card) and OpenRO's words (cream card): K. Errors with a plain sentence and Details: K (same mapping as the old page's agentRow and PLAIN_ERRORS).
- Tone rule: any line matching the old ERROR_LIKE pattern turns coral. This also catches "no model call involved" in the built-in opening line. Kept for parity; flagged below as a defect to decide.
- Build card: "What OpenRO did" head, step rows with Details and result words (Saved, Written, Added, Removed, Done): C. Changed to the tray with stamps and a counter; step rows kept.
- Verdict bands "Passed" / "Failed" / "Not proven yet" with the same copy and Details (raw line and console output): K.
- Working line "OpenRO is working on it. A build can take a minute or two.": K, now with a shimmer.
- Thread error "Could not load the conversation." with Try again: K.
- Notices per chat (kept in session storage, shown only in their chat): K.
- Entrance on new lines: P (new lines rise once; first load does not animate).

Composer and build
- Input: label, placeholder, autocomplete off, id `composer-input`: K.
- Send: disabled while busy: K. Sends the ask; failure texts for 403/413/415 (server words), other failures (the SEND_FAILED sentence) with the words back in the box: K. 502/503 with `ok:false` treated as recorded: K.
- Next ideas chips after a conversation: K (disabled without Studio or while busy).
- Build failure notice in the thread: K. Focus after a build goes back to the same key: N.
- Undo: enabled only when the chat can take a change set back and Studio answers; reason text "Nothing built yet" / "Connect Studio to see what is built" shown beside it: K (shown above the box now, so the input keeps its width).
- Undo confirm: "Undo the last build?" copy, "Keep it" first, "Yes, undo it": K as a MorphingDialog. Focus on open: the first control is Keep it (MorphingDialog focuses the first focusable element). Escape returns focus to Undo (MorphingDialog's own behaviour). Not yet verified: the probe could not open Undo with a built chat in this session.
- Undo results: "Undid the last build. N change(s) were put back.", "Nothing to undo. No build is waiting to be undone in this chat.", the server's own error text: K as thread notices and as Sonner toasts.

In your game (shelf)
- Scripts and Parts sections with counts (SlidingNumber), plain names and kinds (server script, stored script, screen, player script, shared script, in the world, part), raw path behind Details: K.
- Empty text "Nothing built yet. When OpenRO builds something, its scripts and parts show here.": K. Error text "Could not read your game. Check that Studio is connected, then try again.": K.
- Collapse toggle with aria-expanded and label: K.

Pinned hooks (tests/integration.rs and the kit)
- Pinned on the served page's text, now at risk (round 1 result: 85 pass, 3 fail):
  - `id="sidebar"` (test `http_shell_and_no_key_honesty`, line 280): FAILS. The React tree is not in the shell. Decision for the lead: a markup pin the redesign changes on purpose, or a skeleton in the shell.
  - `data-theme="dark"` as the default (test `shell_has_themes_and_settings`, line 566): FAILS. The art file makes LIGHT native; the test pins dark. Decision for the lead, with the reason in a commit.
  - `--faint: #8e8ea5` (test `shell_escapes_game_name_and_meets_contrast`, line 1020): FAILS. Old palette token; Hum uses oklch tokens. Needs a measured contrast check instead of the pin.
  - Not yet reached (the test stops at its first failing assertion): `id="chat-list"`, `id="thread"`, `id="composer"`, `id="status-chip"`, `id="settings"`, `localStorage.setItem('openro-theme'` (the string is in minified JS as double quotes), `localStorage.getItem('openro-theme')` (in the head script: kept), `:focus-visible` (in CSS: kept), `for="composer-input"` (React emits `htmlFor`), `linear-gradient` absence (the Cult UI slab uses a gradient: the recorded exception), `Appearance` text (kept as the label).
- Kept in the shell by placeholder: `__GAME_NAME__` (title and `#server-state`), `__CAN_BUILD__`, `__STUDIO_CONNECTED__`, `__CHIP_CLASS__`, `__CHIP__`, `__KEY_BANNER__` (each once in `index.html`).
- `scripts`/`.loop/checks` and `proof.sh`: not read in round 1 (not run).

## Defects I can see and did not fix in round 1

- The coral "Building it now ..." line: the old ERROR_LIKE rule matches "no model". It is a parity rule with a false positive; decide whether to keep the old rule or narrow it.
- The Rust side still says version 0.2.0 (Cargo.toml); the art file says 0.3.0. Not mine to change.
- The 502 on phone builds in the screenshot run comes from the server's build reply; the page reads it as a failure notice, as the old page did. Not investigated further.
- Tap and contrast measurements are not done (see below).

## What was not checked

- Sloplint (Hallmark render lint) and the house gates (`references/gates.md`), role-coverage script: not run in round 1.
- The unslop review and the impeccable / design-taste pass: not run.
- Reduced-motion run, keyboard-only run of the phone sheet, the undo confirm focus cycle on a built chat, 320-wide overflow, tap-target measurement, contrast measurement on the Hum tokens.
- `data-answers` attributes for the first fold (house rule) are not added yet.
- The update banner and the skip link: not built.
