# Which skill owns the work

Before you act, load the skill that owns the domain. Load at most two supporting skills at a time, and do not read everything that matches. The owning skill decides content, structure, its own intake and its own checks. Ultra Loop decides the bar, the evidence, the review and when the work is done.

If the owning skill wants to ask intake questions, answer them from the request and the bar, record each answer as an assumption, and keep working. Put a question to the owner only when it passes the test in `SKILL.md`.

## Routing table

Skill names are as they appear on the owner's machine. Account skills may carry a plugin prefix, so match on the name. A skill is installed if the host lists it.

| Work | Owner | Supporting |
|---|---|---|
| Landing page or marketing site for a real business | `site-studio` | Claude Design for visual exploration; `frontend-design` |
| Visual design, mockup, prototype, design system | Claude Design: `Artifact` with `action: "quickstart"`, `intent: "design"` (`intent: "other"` for a design system) | `frontend-design`, `impeccable` |
| An existing site, page or product interface | the project's own files, stack and design system, then `interfaces.md` | `impeccable`, `emil-design-eng`, `web-experience`; verify in a browser. `site-studio` only when the owner asks for a redesign |
| Slide deck | `Artifact` quickstart with `intent: "slides"` | `theme-factory` |
| Document for people to read or share | `docs`, or `Artifact` quickstart with `intent: "document"` | `internal-comms` |
| .pdf, .docx, .xlsx, .pptx files | the matching format skill | |
| Feature or bug fix | `superpowers:test-driven-development`; `superpowers:systematic-debugging` or `diagnosing-bugs` for a failure | `ponytail` |
| Code review or audit | `code-review`, `security-review` | `superpowers:receiving-code-review` for feedback you receive |
| Research report from many sources | `deep-research` or `research` | `agent-reach` to fetch from platforms |
| Video, motion, animation | `hyperframes` (the entry point; it routes to the rest) | `media-use` |
| Apotheka video or character work | `apotheka-video`, `brand-character-pipeline`; the project's production rulebook is the authority | |
| Raster images | `chatgpt-image-gen`. If its browser is down, report the images as blocked; do not swap in another generator | `ai-image-prompts` |
| Charts and dashboards | `dataviz` | |
| Deploy or hosting | the platform's connector or command-line tool, after the `run` skill shows it working locally | |
| Claude API or Anthropic SDK work | `claude-api` | |
| An MCP server | `mcp-builder` | |
| Writing or editing a skill | `superpowers:writing-skills` | `skill-creator` |
| Copy that buyers will read | `humanizer` | `conversion-ux` |
| Roblox | `roblox-luau`, `roblox-networking`, `roblox-datastores`; `roblox-art-director` for anything a player sees | |
| Work managed in Paperclip | the `paperclip` skill and its governance skills, where the host has them: they set ownership and approvals | |
| Desktop configuration (Omarchy, Hyprland) | `omarchy` | |

**What the deliverable is.**
- `site-studio` decides what a site deliverable looks like. When it does not say, the deliverable is a working site in a project folder, previewed in a browser.
- A private Artifact is an optional preview the owner can open from any device.
- A Claude Design canvas is for exploring mockups and direction. It is not the shipped site.
- If the host will not publish an Artifact (for example because the page carries a real organisation's name), keep the local files, preview them in a browser, and say so in the report. Do not rename the business to get around the rule.

Claude Design and `site-studio` are the owner's default design method. If `site-studio` is not loaded, use `web-experience` with `frontend-design`, and say in the report that `site-studio` was unavailable.

## Discovery, when no row fits

Stop at the first step that gives you a capable owner.

1. Scan the installed skill list for the nouns and verbs of the domain.
2. Search the account with `SearchSkills` and `SearchPlugins` (load them with `ToolSearch` first). Offer a missing plugin with `SuggestPluginInstall`; never install one silently.
3. Look for deferred tools and connectors with `ToolSearch`, using keywords for the service.
4. Search the open ecosystem with the `find-skills` skill. Read a candidate's SKILL.md from its source and apply it by hand. Installing a third-party skill needs the owner's go-ahead: name it and its source first.
5. For current documentation use web search and fetch. Do this for libraries released after your training, command-line flags, API schemas and prices.
