# OpenRO gaps found while building the game

Each entry: what we tried, what OpenRO did, who owns the fix, status.

| # | Reported by | Gap | Owner | Status |
| --- | --- | --- | --- | --- |
| 1 | openro-core | Open Cloud has no endpoint for game icon, thumbnails, monetisation policy or the age questionnaire (Creator Dashboard only). Universe name, description, visibility and platform flags are supported. | none (Roblox limit); OpenRO publish checklist tells the user the manual steps | documented |
| 2 | world | Blender tool only accepts 5 fixed presets; torii_gate is 180 tris, no UV, no texture. No way to get stalls, portals, arenas, trees, NPCs or textured props. Needs a structured-description or agent-written bpy tool with smart UV, baked colour x AO PNG, separate glow mesh, manifest (bbox studs, tris, anchors), 2-angle previews, hard fail above 20k tris. Reference recipe: swarm-v011/world-prototype. | openro-assets | open |
