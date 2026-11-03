import { afterEach, describe, expect, it } from 'vitest';
import { McpStdioClient } from '../../electron/mcp';

// Runs only when OPENRO_MCP_INTEGRATION=1 (see vitest.config.mts). Set
// OPENRO_MCP_EVERYTHING to a local server-everything dist/index.js to skip npx.
function serverSpec(): { command: string; args: string[] } {
  const local = process.env.OPENRO_MCP_EVERYTHING;
  if (local) {
    return { command: process.execPath, args: [local, 'stdio'] };
  }
  return { command: process.platform === 'win32' ? 'npx.cmd' : 'npx', args: ['-y', '@modelcontextprotocol/server-everything@2.1.0', 'stdio'] };
}

describe('MCP reference server (@modelcontextprotocol/server-everything)', () => {
  const client = new McpStdioClient(60_000);
  afterEach(() => client.stop());

  it('completes the handshake, lists tools, and echoes text containing U+2028 unchanged', async () => {
    await client.start(serverSpec());
    expect(client.running).toBe(true);
    expect(client.identity?.session).toMatch(/^[0-9a-f-]{36}$/);

    const tools = client.listTools();
    const echo = tools.find((tool) => tool.name === 'echo');
    expect(echo?.inputSchema).toMatchObject({ type: 'object', properties: { message: { type: 'string' } } });

    const message = 'line one\u2028line two \u2029 end';
    const reply = await client.callTool('echo', { message });
    expect(reply).toBe(`Echo: ${message}`);
  }, 120_000);
});
