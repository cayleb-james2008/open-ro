# Untitled Anime Card Game

A Roblox card-collecting and 4v4 auto-battler, built and shipped with OpenRO. This folder is a normal OpenRO project (Rojo layout), so you can open it in OpenRO, sync it into Studio, playtest it, and publish it.

## Layout

| Folder | Goes to |
| --- | --- |
| `src/server` | `ServerScriptService.Server` |
| `src/client` | `StarterPlayer.StarterPlayerScripts.Client` |
| `src/shared` | `ReplicatedStorage.Shared` |
| `ui` | `StarterGui.Interface` |
| `assets` | art, meshes and audio sources |
| `docs` | design notes and `OPENRO-GAPS.md` |

## Tools

`rojo`, `selene` and `stylua` (config in `selene.toml` and `stylua.toml`).

```sh
rojo build default.project.json -o build/UntitledAnimeCardGame.rbxl
selene src
stylua --check src
```

`build/`, `art/references/` and `refs/` are never committed.

## Status

Under active construction for OpenRO 1.0. `docs/OPENRO-GAPS.md` lists every place OpenRO fell short while building this game.
