import { describe, expect, it } from 'vitest';
import { providerOptions } from '../electron/providerCatalog';
import { approvalCopy, providerInitials } from '../src/labels';

describe('provider monograms', () => {
  it('ignores parenthetical notes and punctuation', () => {
    expect(providerInitials('Ollama (local)')).toBe('Ol');
    expect(providerInitials('llama.cpp server (local)')).toBe('LC');
    expect(providerInitials('OpenAI API key')).toBe('OA');
    expect(providerInitials('ChatGPT subscription (Codex CLI)')).toBe('CS');
    expect(providerInitials('')).toBe('OP');
  });

  it('gives every catalog route two letters or digits', () => {
    for (const option of providerOptions) {
      expect(providerInitials(option.name)).toMatch(/^[\p{L}\p{N}]{2}$/u);
    }
  });
});

describe('approval card copy', () => {
  it('shows the exact MCP tool and arguments', () => {
    const copy = approvalCopy({ id: '1', kind: 'mcp', createdAt: '', mcpTool: 'run_code', mcpArgs: '{"command":"print(1)"}' });
    expect(copy).toEqual({
      title: 'Review MCP tool: run_code',
      detail: 'The tool runs on the connected MCP server only if you approve these exact arguments',
      preview: '{\n  "command": "print(1)"\n}',
      approveLabel: 'Run tool',
    });
  });

  it('shows the exact server command, quoting arguments with spaces', () => {
    const copy = approvalCopy({ id: '2', kind: 'mcp-server', createdAt: '', command: 'npx', serverArgs: ['-y', 'my server'] });
    expect(copy.preview).toBe('npx -y "my server"');
    expect(copy.approveLabel).toBe('Start server');
  });
});
