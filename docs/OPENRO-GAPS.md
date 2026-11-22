# OpenRO gaps and improvements found while building the game

Each entry: type (`gap` or `improve`), what we tried / user value + time saved, who owns it, status.

| # | Type | Reported by | Details | Owner | Status |
| --- | --- | --- | --- | --- | --- |
| 1 | gap | openro-core | Open Cloud has no endpoint for game icon, thumbnails, monetisation policy or the age questionnaire (Creator Dashboard only). Universe name, description, visibility and platform flags are supported. | none (Roblox limit); OpenRO publish checklist tells the user the manual steps | documented |
| 2 | gap | world | Blender tool only accepts 5 fixed presets; torii_gate is 180 tris, no UV, no texture. No way to get stalls, portals, arenas, trees, NPCs or textured props. Needs a structured-description or agent-written bpy tool with smart UV, baked colour x AO PNG, separate glow mesh, manifest (bbox studs, tris, anchors), 2-angle previews, hard fail above 20k tris. Reference recipe: swarm-v011/world-prototype. | openro-assets | open |
| 3 | gap | game-ui | With only a 3B local model, `/api/chat` plan mode timed out after about 170 s with nothing built. No director can build until the Hydrogen provider lands. | openro-agent | open |
| 4 | gap | game-ui | v0.10 has no project concept: no /api/projects; agent tools are place-only, so a Rojo project's src/client files cannot be read or written by chat. | openro-core (API), openro-agent (tools) | open |
| 5 | gap | game-ui | `/api/chat` holds one HTTP request open for the whole run: no run id, no stream to poll, no resume. Long builds cannot be backgrounded. | openro-agent | open |
| 6 | gap | art | Image generation is card-shaped only (text-only prompt): no reference images, free size, output path, or non-card kinds (UI icon, frame, store thumbnail). Needs `/api/art/generate` with 1-N references, free prompt, size, out path under art/, PNG + metadata, structured refusal result. | openro-assets | open |
| 7 | gap | art | No image post-processing tool: exact crop/resize, alpha composite, rounded masks, 9-slice + slices.json, quantize/compress, 5 MB + decode check, sha256, art log row. | openro-assets | open |
| 8 | gap | art | Image models return opaque PNGs; frames, icons and 9-slice skins need transparency (background removal / colour-key / deterministic drawing). Recipe: swarm-v011/art-notes/withdrawn-handmade-prototype/tools/art. | openro-assets | open |
| 9 | gap | art | After upload nothing generates src/shared/AssetIds.luau from assets.lock.json. Default `assets.authorised` must add art/frames/*/*.png and art/store/*.png (art/frames/preview/* stays out). | openro-assets | open |
