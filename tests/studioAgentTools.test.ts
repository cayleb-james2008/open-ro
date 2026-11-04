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

  it('advertises keyboard and mouse input schemas with actions and a studio id', () => {
    for (const name of ['studio_keyboard', 'studio_mouse']) {
      const schema = toolSchemas().find((tool) => tool.name === name);
      expect(schema).toBeDefined();
      expect(schema?.parameters.required).toEqual(['actions', 'studioId']);
      expect(schema?.parameters.properties.actions?.type).toBe('array');
      expect(schema?.parameters.properties.studioId?.type).toBe('string');
    }
  });

  it('advertises studio_capture with a required studio id and optional capture id', () => {
    const schema = toolSchemas().find((tool) => tool.name === 'studio_capture');
    expect(schema).toBeDefined();
    expect(schema?.parameters.required).toEqual(['studioId']);
    expect(schema?.parameters.properties.captureId?.type).toBe('string');
    expect(schema?.parameters.required).not.toContain('captureId');
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

  it('labels keyboard and mouse input with action previews', () => {
    const keyboard = approvalCopy(view({
      studioOp: 'input',
      mcpTool: 'user_keyboard_input',
      mcpArgs: JSON.stringify({ actions: [{ action: 'keyPress', key_code: 'Space' }, { action: 'textInput', text_inputs: 'hello' }] }),
    }));
    expect(keyboard.title).toBe('Review Studio keyboard input');
    expect(keyboard.detail).toContain('2 step(s)');
    expect(keyboard.preview).toContain('keyPress Space');
    expect(keyboard.preview).toContain('textInput "hello"');
    expect(keyboard.approveLabel).toBe('Send input');

    const mouse = approvalCopy(view({
      studioOp: 'input',
      mcpTool: 'user_mouse_input',
      mcpArgs: JSON.stringify({ actions: [{ action: 'mouseButtonClick', mouse_button: 'left', x: 10, y: 20 }] }),
    }));
    expect(mouse.title).toBe('Review Studio mouse input');
    expect(mouse.preview).toContain('(10, 20)');
    expect(mouse.detail).not.toMatch(/script|write/i);

    const malformed = approvalCopy(view({ studioOp: 'input', mcpTool: 'user_keyboard_input', mcpArgs: 'not json' }));
    expect(malformed.preview).toBe('');
    expect(malformed.title).toBe('Review Studio keyboard input');
  });

  it('counts every staged step and previews buttons, targets, and escaped text', () => {
    const actions = Array.from({ length: 21 }, () => ({ action: 'keyPress', key_code: 'Space' }));
    const many = approvalCopy(view({ studioOp: 'input', mcpTool: 'user_keyboard_input', mcpArgs: JSON.stringify({ actions }) }));
    expect(many.detail).toContain('21 step(s)');
    expect(many.preview.split('\n')).toHaveLength(21);

    const rightClick = approvalCopy(view({
      studioOp: 'input',
      mcpTool: 'user_mouse_input',
      mcpArgs: JSON.stringify({ actions: [{ action: 'mouseButtonClick', mouse_button: 'right', instance_path: 'game.Workspace.Button' }] }),
    }));
    expect(rightClick.preview).toContain('right');
    expect(rightClick.preview).toContain('@ game.Workspace.Button');

    const text = 'quote " and\nnewline';
    const typed = approvalCopy(view({
      studioOp: 'input',
      mcpTool: 'user_keyboard_input',
      mcpArgs: JSON.stringify({ actions: [{ action: 'textInput', text_inputs: text }] }),
    }));
    expect(typed.preview).toContain(JSON.stringify(text));
    expect(typed.preview).not.toContain('\nnewline');
  });

  it('labels viewport capture and never claims a script write', () => {
    const copy = approvalCopy(view({ studioOp: 'capture', mcpTool: 'screen_capture', mcpArgs: JSON.stringify({ capture_id: 'shot-1', studio_id: 'S1' }) }));
    expect(copy.title).toBe('Review Studio viewport capture');
    expect(copy.detail).toContain('viewport');
    expect(copy.approveLabel).toBe('Capture viewport');
    expect(copy.detail).not.toMatch(/script|write/i);
  });
});
