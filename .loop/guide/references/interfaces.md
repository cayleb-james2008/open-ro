# Interfaces, and the skills the owner named

If the project has a design system, a design pack or a house library file (for example a `FRONTEND-LIBRARY.md`), read it first. It outranks this file and every installed skill, and its thresholds replace the numbers here. Do not re-pick a look the owner already chose. This file stands in for any skill below that is not installed.

| Step | Skill, when installed, and how to call it |
|---|---|
| Scope each change | `ponytail` |
| Direction, once per surface | `impeccable` with a command and a target, such as `audit <target>`; never bare, because a bare call shows a menu and waits. Taste (`design-taste-frontend`) on landing and marketing pages only |
| Build | `impeccable`: read its craft floor before editing |
| Motion, last | Emil Kowalski's `emil-design-eng`, with a concrete question about one animation |
| Review a render | unslop, meaning the review of a rendered screen for machine-made signs, not a prose editor |

Where a skill pauses to ask, answer from the bar and note the assumption. This loop's limits replace a skill's own round limits. Where skills disagree, the stricter rule wins and the owner's brief settles looks.

**Mode.** Name what the surface is for: to persuade (a page that sells), to operate (a tool people work in), to read (long text) or to be an experience (a showpiece). A tool gets restraint.

**Direction.** At the first change to a surface, add a block headed "DIRECTION v1" to `.loop/BAR.md`: what the product does and for whom; two or three concrete words for its character (never clean, modern, sleek, minimal or premium); the place or material it belongs to; the one or two spots that get rich detail; one signature detail. Keep an existing brand and build around it. A change of direction is a new block that names what it replaces. On a landing page also write one line on what kind of page it is and for whom, keep the opening screen to a headline of at most two lines, a short supporting line and one main action, and never use three equal cards in a row.

**Order of work.** Make every state work and pass the script gates before any polish. Decide motion last. The review of the render is a hunt lens: a fresh finder lists each machine-made sign with its location. No signs: say so and change nothing. A few isolated signs: fix only those, and keep the palette, type and layout. Many signs: fix in nine passes, each across the whole screen. Each pass is one backlog row and one commit (`B12: spacing pass`):

1. spacing: one 4-pixel grid; the same thing the same size; less space inside a group than between groups
2. colour: one tinted neutral ramp, three text roles, one accent used sparingly
3. icons: one set at one size; no emoji
4. copy: what the product does for whom, in plain words, from facts already present
5. type: a few sizes, weights 400 to 600, sentence case, body text at 16 pixels or more
6. layout: keep the sections and their order; change proportions only for clarity
7. details: radii, borders and shadows from one system; no card inside a card
8. visuals: show the product or one true thing from its world, never filler
9. motion: first count how often people see it. Used a hundred times a day or triggered from the keyboard: no animation. Otherwise only where it explains something, with one easing curve; press feedback 100 to 160 milliseconds, menus 150 to 250, dialogs up to 500; exits faster than entrances

Each sign that a script can detect gets a script check; the rest become `judge` rows with an example pair. Add one `judge` condition to the bar for every surface: "would this screen still fit another product's name?" A bare grey page fails it as surely as a gaudy one. Where a fact is missing, leave a marked slot for the owner; never invent numbers, testimonials or claims. Never remove a section, a navigation element or an action the documents name.

**Script gates** (`cmd` conditions, added as the hunt reproduces violations). Run them on the rendered page at 320, 390 and 1440 CSS pixels wide, once with default motion and once with reduced motion, and with worst-case data such as long names, empty lists and corrupted stored data:

- no console errors; the page is never wider than the viewport; no text cut off by a box that hides overflow
- contrast at least 4.5 to 1 for all text under 24 pixels (under 18.66 when bold), placeholders included, and 3 to 1 for larger text, for focus rings, and for the edges and icons that are the only way to see a control
- pointer targets at least 24 by 24 CSS pixels (links inside a sentence exempt), 44 by 44 at the two narrow widths; every control usable by keyboard with visible focus; inputs labelled
- each control and data view has the states the bar lists for it (hover, focus, disabled, loading, error, empty)
- computed transitions never on `all` or on layout properties (width, height, margin, padding, top, left); no `ease-in` used alone; nothing scaling from zero; no scale on hover; no endless animation except a named loading indicator; in the reduced-motion run, nothing moves
- user-supplied text is never rendered as markup
- lab performance on the measuring tool's default mobile profile, labelled lab, median of three runs: largest paint within 2.5 seconds, layout shift within 0.1, total blocking time within 200 milliseconds as the lab stand-in for interaction delay

A source scan flags gradients between hues, gradient text, glows, decorative glass and headings heavier than 600 for the review; a script does not fail them. Serve the build yourself and confirm which build you are measuring. Capture after the page settles, look at the whole page, and open every image you cite: a screenshot of the wrong state or theme is a confident false pass. A picture you send to the owner shows his real configuration and data, or says "example" inside the image: he reads a screenshot as his live system. On looks, a model can fail or flag a screen and can pass only a `judge` condition with its example pair. How the result looks to the owner is an `owner` condition.

Condensed, with changes, from: unslop by Damir Sokolovsky (CC BY 4.0, creativecommons.org/licenses/by/4.0, github.com/heydamir/unslop), by way of the owner's UNSLOP-REVIEW-GATE; impeccable by Paul Bakaus (Apache-2.0, github.com/pbakaus/impeccable); the design engineering skill by Emil Kowalski (MIT, github.com/emilkowalski/skills); taste-skill by Leonxlnx (MIT, github.com/Leonxlnx/taste-skill). Rule 4 of the skill is condensed from ponytail by Dietrich Gebert (MIT, github.com/DietrichGebert/ponytail).
