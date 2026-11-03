`npm test` runs the unit tests in this folder. They need no network, model, or Roblox Studio.

`tests/integration/` talks to a real MCP server, `@modelcontextprotocol/server-everything` 2.1.0, and runs only when you ask for it:

```sh
OPENRO_MCP_INTEGRATION=1 npx vitest run tests/integration
```

That fetches the server with `npx`. To use a copy you already have, also set `OPENRO_MCP_EVERYTHING=/path/to/server-everything/dist/index.js`.

Testing Studio, packaging, and Linux Studio through Wine/Vinegar still needs real Windows and Arch/Omarchy machines. See `docs/RELEASE-GATES.md`.
