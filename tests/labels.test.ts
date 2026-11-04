import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import type { ComfyGenerationPlan } from '../electron/comfyClient';
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

  it('describes a local media generation without MCP wording', () => {
    const copy = approvalCopy({
      id: 'm',
      kind: 'mcp',
      createdAt: '',
      mcpTool: 'media_generate',
      mcpServer: 'openro-media',
      mcpArgs: JSON.stringify({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'Late night trap', durationSeconds: 5, seed: 7 }),
    });
    expect(copy).toEqual({
      title: 'Generate local audio: audio-ace-step-1.5-turbo',
      detail: '5s, seed 7 · runs on your loopback ComfyUI server only if you approve; nothing is downloaded',
      preview: 'Late night trap',
      approveLabel: 'Generate audio',
    });
    const malformed = approvalCopy({ id: 'm2', kind: 'mcp', createdAt: '', mcpTool: 'media_generate', mcpServer: 'openro-media', mcpArgs: 'not json' });
    expect(malformed.preview).toBe('');
    expect(malformed.title).toContain('ComfyUI audio');
    const nullArgs = approvalCopy({ id: 'm3', kind: 'mcp', createdAt: '', mcpTool: 'media_generate', mcpServer: 'openro-media', mcpArgs: 'null' });
    expect(nullArgs.preview).toBe('');
    const foreignMcp = approvalCopy({ id: 'm4', kind: 'mcp', createdAt: '', mcpTool: 'media_generate', mcpServer: 'some-other-server', mcpArgs: '{}' });
    expect(foreignMcp.title).toBe('Review MCP tool: media_generate');
  });
});

describe('local media approval', () => {
  const plan: ComfyGenerationPlan = {
    planId: 'plan-1',
    presetId: 'audio-ace-step-1.5-turbo',
    modelId: 'ace-step-1.5-turbo',
    prompt: 'Late night trap',
    lyrics: '[Instrumental]',
    durationSeconds: 5,
    seed: 3,
    workflow: { '94': { class_type: 'TextEncodeAceStepAudio1.5', inputs: { tags: 'Late night trap' } } },
    summary: 'ACE-Step 1.5 Turbo: 5s, 8 steps, seed 3.',
    createdAt: '2026-11-05T00:00:00.000Z',
  };

  it('freezes the plan behind an MCP-shaped approval', () => {
    const queue = new ApprovalQueue(() => 1_000);
    const item = queue.stageMedia({ projectRoot: 'C:\\Game', plan });
    expect(item).toMatchObject({ kind: 'mcp', mcpTool: 'media_generate', projectRoot: 'C:\\Game' });
    expect(item.mcpServer).toMatchObject({ command: 'openro-media' });
    expect(item.mediaPlan).not.toBe(plan);
    plan.prompt = 'mutated after staging';
    expect(item.mediaPlan?.prompt).toBe('Late night trap');
    expect(JSON.parse(item.mcpArgs ?? '{}')).toMatchObject({
      presetId: 'audio-ace-step-1.5-turbo',
      modelId: 'ace-step-1.5-turbo',
      prompt: 'Late night trap',
      durationSeconds: 5,
      seed: 3,
    });
    const taken = queue.take(item.id, 'mcp');
    expect(taken.mediaPlan?.planId).toBe('plan-1');
  });

  it('is wired through the agent tool, approval execution, UI and production sink', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("name: 'media_generate'");
    expect(main).toContain('[...toolSchemas(), MEDIA_TOOL_SCHEMA,');
    expect(main).toContain("if (String(name) === 'media_generate')");
    expect(main).toContain('const plan = buildComfyPlan({');
    expect(main).toContain('Stage only. Health is checked by the approve-time execution');
    expect(main).toContain('approvals.stageMedia({ projectRoot: root, plan })');
    expect(main).toContain("if (item.kind === 'mcp' && item.mediaPlan)");
    expect(main).toContain("path.join(app.getPath('userData'), 'media')");
    expect(main).toContain('runComfyPlan(comfyBaseUrl(), item.mediaPlan, path.join(app.getPath(\'userData\'), \'media\'))');
    expect(main).toContain("'Rejected the local media generation. Nothing ran.'");
    const app = readFileSync('src/App.tsx', 'utf8');
    expect(app).toContain('<span>Local media</span>');
    expect(app).toContain('api.comfyStatus()');
    expect(app).toContain('api.prepareMedia');
    expect(app).toContain('api.generateMedia');
    expect(app).toContain('Refresh ComfyUI');
    expect(app).toContain('Generate audio');
    expect(app).toContain('resolvingApprovals');
    expect(app).toContain('disabled={resolving}');
    expect(app).toContain('Frozen plan:');
    expect(app).toContain('setMediaPlan(null);');
    const labels = readFileSync('src/labels.ts', 'utf8');
    expect(labels).toContain("item.mcpServer === 'openro-media'");
    expect(labels).toContain('Generate local audio');
  });
});
