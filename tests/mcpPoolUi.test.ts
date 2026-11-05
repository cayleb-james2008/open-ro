import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
const styles = readFileSync('src/styles.css', 'utf8').replace(/\r\n/g, '\n');

describe('Multi-Server MCP UI: Structure & Components in App.tsx', () => {
  it('renders the MCP server pool health bar with active count and actions', () => {
    expect(app).toContain('className="mcp-pool-health-bar"');
    expect(app).toContain('MCP Server Pool');
    expect(app).toContain('mcpPoolHealth?.activeCount');
    expect(app).toContain('+ Add MCP Server');
    expect(app).toContain('Stop all servers');
  });

  it('renders the multi-server grid with server cards and lifecycle actions', () => {
    expect(app).toContain('className="mcp-server-grid"');
    expect(app).toContain('mcp-server-card');
    expect(app).toContain('server-cmd-preview');
    expect(app).toContain('server-stats-row');
    expect(app).toContain('connectPoolServer(server)');
    expect(app).toContain('disconnectPoolServer(server.id)');
    expect(app).toContain('reconnectPoolServer(server.id)');
    expect(app).toContain('deletePoolServer(server.id)');
  });

  it('provides an Add MCP Server modal with 1-click presets', () => {
    expect(app).toContain('mcpModalOpen');
    expect(app).toContain('Configure MCP Server');
    expect(app).toContain('Roblox Studio');
    expect(app).toContain('Blender 3D');
    expect(app).toContain('Filesystem');
    expect(app).toContain('saveModalServer()');
  });

  it('renders the searchable and filterable MCP tool browser', () => {
    expect(app).toContain('Discovered MCP Tools');
    expect(app).toContain('tool-search-input');
    expect(app).toContain('tool-server-select');
    expect(app).toContain('item.agentToolName');
    expect(app).toContain('item.directAgentToolName');
  });
});

describe('Multi-Server MCP UI: CSS Styles in styles.css', () => {
  it('defines required stylesheet rules for grid, cards, tags, and filters', () => {
    expect(styles).toContain('.mcp-pool-health-bar');
    expect(styles).toContain('.mcp-server-grid');
    expect(styles).toContain('.mcp-server-card');
    expect(styles).toContain('.status-tag');
    expect(styles).toContain('.status-tag.connected');
    expect(styles).toContain('.status-tag.hibernating');
    expect(styles).toContain('.status-tag.error');
    expect(styles).toContain('.tool-search-input');
    expect(styles).toContain('.tool-server-select');
    expect(styles).toContain('.mcp-preset-buttons');
  });
});
