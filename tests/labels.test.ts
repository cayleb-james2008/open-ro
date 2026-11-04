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
      kind: 'media',
      createdAt: '',
      mediaPreset: 'audio-ace-step-1.5-turbo',
      mediaPrompt: 'Late night trap',
      mediaDuration: 5,
      mediaSeed: 7,
    });
    expect(copy).toEqual({
      title: 'Generate local audio: audio-ace-step-1.5-turbo',
      detail: '5s, seed 7 · runs on your loopback ComfyUI server only if you approve; nothing is downloaded',
      preview: 'Late night trap',
      approveLabel: 'Generate audio',
    });
    const empty = approvalCopy({ id: 'm2', kind: 'media', createdAt: '' });
    expect(empty.preview).toBe('');
    expect(empty.title).toContain('ComfyUI audio');
    const foreignMcp = approvalCopy({ id: 'm4', kind: 'mcp', createdAt: '', mcpTool: 'media_generate', mcpServer: 'openro-media', mcpArgs: '{}' });
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

  it('freezes the plan behind a first-class media approval', () => {
    const queue = new ApprovalQueue(() => 1_000);
    const item = queue.stageMedia({ projectRoot: 'C:\\Game', plan });
    expect(item).toMatchObject({
      kind: 'media',
      projectRoot: 'C:\\Game',
      mediaPreset: 'audio-ace-step-1.5-turbo',
      mediaPrompt: 'Late night trap',
      mediaDuration: 5,
      mediaSeed: 3,
    });
    expect(item.mcpTool).toBeUndefined();
    expect(item.mcpArgs).toBeUndefined();
    expect(item.mcpServer).toBeUndefined();
    expect(item.mediaPlan).not.toBe(plan);
    plan.prompt = 'mutated after staging';
    plan.workflow['94'].inputs.tags = 'mutated workflow';
    expect(item.mediaPlan?.prompt).toBe('Late night trap');
    expect(item.mediaPlan?.workflow['94'].inputs.tags).toBe('Late night trap');
    const taken = queue.take(item.id, 'media');
    expect(taken.mediaPlan?.planId).toBe('plan-1');
  });

  it('is wired through the agent tool, approval execution, UI and production sink', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("name: 'media_generate'");
    expect(main).not.toContain("as ToolSchema['name']");
    expect(main).toContain('[...toolSchemas(), MEDIA_TOOL_SCHEMA,');
    expect(main).toContain("case 'media_generate':");
    expect(main).toContain('const plan = buildComfyPlan({');
    expect(main).toContain('Stage only. Health is checked by the approve-time execution');
    expect(main).toContain('approvals.stageMedia({ projectRoot: root, plan })');
    expect(main).toContain("pendingApproval: { kind: 'media'");
    expect(main).toContain("if (item.kind === 'media')");
    expect(main).toContain("path.join(app.getPath('userData'), 'media')");
    expect(main).toContain('runComfyPlan(comfyBaseUrl(), item.mediaPlan, path.join(app.getPath(\'userData\'), \'media\'))');
    expect(main).toContain("'Rejected the local media generation. Nothing ran.'");
    expect(main).toContain('mediaPreset: item.mediaPreset');
    expect(main).toContain('mediaPrompt: item.mediaPrompt');
    expect(main).toContain('mediaDuration: item.mediaDuration');
    expect(main).toContain('mediaSeed: item.mediaSeed');
    const types = readFileSync('electron/types.ts', 'utf8');
    expect(types).toContain("| 'media_generate'");
    expect(types).toContain("studio' | 'delete' | 'media'");
    expect(types).toContain('mediaPreset?: string');
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
    expect(labels).toContain("case 'media':");
    expect(labels).not.toContain("openro-media");
    expect(labels).toContain('Generate local audio');
  });
});
