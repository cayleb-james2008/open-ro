import { describe, expect, it } from 'vitest';
import { toolSchemas } from '../electron/agent';
import type { PendingApprovalView } from '../electron/types';
import { approvalCopy } from '../src/labels';

const view = (overrides: Partial<PendingApprovalView>): PendingApprovalView => ({
  id: 'approval-1',
  kind: 'studio',
  createdAt: '2026-11-04T00:00:00.000Z',
  ...overrides,
});

describe('agent-facing Studio playtest and console tools', () => {
  it('advertises studio_playtest with a boolean isStart and a studio id', () => {
    const schema = toolSchemas().find((tool) => tool.name === 'studio_playtest');
    expect(schema).toBeDefined();
    expect(schema?.parameters.required).toEqual(['isStart', 'studioId']);
    expect(schema?.parameters.properties.isStart?.type).toBe('boolean');
    expect(schema?.parameters.properties.studioId?.type).toBe('string');
  });

  it('advertises studio_console with a studio id', () => {
    const schema = toolSchemas().find((tool) => tool.name === 'studio_console');
    expect(schema).toBeDefined();
    expect(schema?.parameters.required).toEqual(['studioId']);
  });

  it('labels playtest start and stop with the staged flag', () => {
    const start = approvalCopy(view({ studioOp: 'play', mcpArgs: JSON.stringify({ is_start: true, studio_id: 'S1' }) }));
    expect(start.title).toBe('Review Studio playtest: start');
    expect(start.approveLabel).toBe('Start playtest');
    const stop = approvalCopy(view({ studioOp: 'play', mcpArgs: JSON.stringify({ is_start: false, studio_id: 'S1' }) }));
    expect(stop.title).toBe('Review Studio playtest: stop');
    expect(stop.approveLabel).toBe('Stop playtest');
  });

  it('labels console reads and never describes a script write', () => {
    const copy = approvalCopy(view({ studioOp: 'console', mcpArgs: JSON.stringify({ studio_id: 'S1' }) }));
    expect(copy.title).toBe('Review Studio console read');
    expect(copy.approveLabel).toBe('Read output');
    expect(copy.detail).not.toMatch(/script|write/i);
  });

  it('defaults to start wording when staged arguments are unreadable', () => {
    const copy = approvalCopy(view({ studioOp: 'play', mcpArgs: 'not json' }));
    expect(copy.approveLabel).toBe('Start playtest');
  });
});
