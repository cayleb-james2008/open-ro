# Optional integrations and extension model

Open-Ro has one required dependency: a usable model connection. Every other capability is optional. No extension should require an Open-Ro account or route user traffic through an Open-Ro-operated service.

## Capability categories

| Capability | Local dependency | External account/key | Status in 0.1.0 |
|---|---|---|---|
| Roblox Studio | Roblox Studio with built-in MCP server | Roblox account for Studio itself | Generic MCP stdio client exists and was tested against the MCP reference server, not against Studio; Windows setup documented; Linux Wine/Vinegar experimental |
| Rojo | Rojo CLI and Studio plugin | None | Detection and setup guidance; project sync workflow planned |
| Blender | Blender desktop executable | None | Background Python task runner implemented; complete mesh/asset transfer UX planned |
| Git | Git CLI | None for local work | Detection and guidance; checkpoint/diff UI planned |
| Images | A user-selected image model/API | Depends on provider | Planned |
| 3D generation | Blender and optionally a generation API | Depends on provider | Planned |
| General MCP servers | MCP stdio process | Depends on each server | One active stdio server at a time; dynamic installer/registry planned |

## Provider setup principle

A capability's install card must include: what it does; local dependencies; account/key requirements; free/paid status; official setup link; supported OSes; permission scope; and a connection test. Users can skip every card and still use the chat workspace with a model connection.

## Extension manifest (planned contract)

The following shape is the proposed declarative metadata for community extensions. It is documentation only in 0.1.0; the app does not yet install manifests dynamically.

```json
{
  "id": "org.example.documentation-mcp",
  "name": "Documentation MCP",
  "version": "1.0.0",
  "description": "Search local or hosted API documentation.",
  "platforms": ["win32", "linux"],
  "install": {
    "kind": "npm-global",
    "package": "@example/docs-mcp"
  },
  "launch": {
    "command": "docs-mcp",
    "args": []
  },
  "permissions": ["network", "mcp-tools"],
  "setup": {
    "signupUrl": "https://example.com/signup",
    "credentialNames": ["EXAMPLE_API_KEY"]
  }
}
```

Dynamic installation is intentionally not enabled yet. The eventual installer must show the exact command, source package, publisher, requested permissions and credential names, require approval, and support disable/uninstall. Secrets will be stored in the OS credential store, not extension manifests.

## Extension safety

MCP processes run locally with the user's OS permissions and may read/write whatever their own implementation allows. Open-Ro must not silently install a community server or grant it access to browser profiles, cloud credentials, Roblox cookies or arbitrary folders. New servers should be disabled until explicitly connected.
