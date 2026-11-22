# OpenRO gaps found while building the game

Each entry: what we tried, what OpenRO did, who owns the fix, status.

| # | Reported by | Gap | Owner | Status |
| --- | --- | --- | --- | --- |
| 1 | openro-core | Open Cloud has no endpoint for game icon, thumbnails, monetisation policy or the age questionnaire (Creator Dashboard only). Universe name, description, visibility and platform flags are supported. | none (Roblox limit); OpenRO publish checklist tells the user the manual steps | documented |
| 2 | world | Blender tool only accepts 5 fixed presets; torii_gate is 180 tris, no UV, no texture. No way to get stalls, portals, arenas, trees, NPCs or textured props. Needs a structured-description or agent-written bpy tool with smart UV, baked colour x AO PNG, separate glow mesh, manifest (bbox studs, tris, anchors), 2-angle previews, hard fail above 20k tris. Reference recipe: swarm-v011/world-prototype. | openro-assets | open |
| 3 | game-ui | With only a 3B local model, `/api/chat` plan mode timed out after about 170 s with nothing built. No director can build until the Hydrogen provider lands. | openro-agent | open |
| 4 | game-ui | v0.10 has no project concept: no /api/projects; agent tools are place-only, so a Rojo project's src/client files cannot be read or written by chat. | openro-core (API), openro-agent (tools) | open |
| 5 | game-ui | `/api/chat` holds one HTTP request open for the whole run: no run id, no stream to poll, no resume. Long builds cannot be backgrounded. | openro-agent | open |
