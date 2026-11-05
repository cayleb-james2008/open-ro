import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { approvalCopy } from '../src/labels';

describe('Blender Task Approval Flow & IPC Surface', () => {
  it('stageBlenderTask creates a pending approval with kind: blender-task using object parameter', () => {
    const queue = new ApprovalQueue();
    const item = queue.stageBlenderTask({
      projectRoot: '/test-project',
      taskType: 'decimate_mesh',
      inputFile: 'models/sword.obj',
      outputDir: 'models',
      params: { targetTriangles: 10000 },
      description: 'Decimate sword mesh to 10k tris',
      taskDescription: 'decimate_mesh (models/sword.obj)',
    });

    expect(item.kind).toBe('blender-task');
    expect(item.blenderTaskType).toBe('decimate_mesh');
    expect(item.projectRoot).toBe('/test-project');
    expect(item.blenderInputFile).toBe('models/sword.obj');
    expect(item.blenderParams).toEqual({ targetTriangles: 10000 });
  });

  it('stageBlenderTask supports positional arguments calling convention', () => {
    const queue = new ApprovalQueue();
    const item = queue.stageBlenderTask(
      'Convert format to FBX',
      'OBJ to FBX with coordinate transform',
      'assets/models',
      ['models/tree.obj'],
      '/project',
    );

    expect(item.kind).toBe('blender-task');
    expect(item.blenderTaskDescription).toBe('Convert format to FBX');
    expect(item.blenderScriptSummary).toBe('OBJ to FBX with coordinate transform');
    expect(item.blenderOutputDir).toBe('assets/models');
    expect(item.blenderTargetFiles).toEqual(['models/tree.obj']);
    expect(item.projectRoot).toBe('/project');
  });

  it('approvalCopy generates informative title, detail, and preview for blender-task', () => {
    const copy = approvalCopy({
      id: 'app-1',
      kind: 'blender-task',
      createdAt: new Date().toISOString(),
      blenderTaskType: 'convert_format',
      blenderTargetFiles: ['models/coin.obj'],
      blenderDescription: 'Convert OBJ to FBX (Y-up)',
      blenderParams: { targetFormat: 'fbx' },
    });

    expect(copy.title).toContain('Review Blender 3D Task: convert_format');
    expect(copy.detail).toContain('models/coin.obj');
    expect(copy.detail).toContain('runs only if you approve');
    expect(copy.approveLabel).toBe('Run 3D task');
    expect(copy.preview).toContain('"targetFormat": "fbx"');
  });

  it('approvalCopy handles script summary preview when provided', () => {
    const copy = approvalCopy({
      id: 'app-2',
      kind: 'blender-task',
      createdAt: new Date().toISOString(),
      blenderTaskType: 'decimate_mesh',
      blenderInputFile: 'shield.fbx',
      blenderScriptSummary: 'Collapsing edges targeting 5,000 triangles',
    });

    expect(copy.title).toContain('decimate_mesh');
    expect(copy.preview).toBe('Collapsing edges targeting 5,000 triangles');
  });

  it('ApprovalQueue takes blender-task and marks it consumed', () => {
    const queue = new ApprovalQueue();
    const item = queue.stageBlenderTask({
      projectRoot: '/test',
      outputDir: 'out',
      taskDescription: 'test task',
    });

    expect(queue.list()).toHaveLength(1);
    const taken = queue.take(item.id, 'blender-task');
    expect(taken.id).toBe(item.id);
    expect(queue.list()).toHaveLength(0);
    expect(() => queue.take(item.id)).toThrow('That approval is no longer pending.');
  });

  it('preload.ts INVOKE_CHANNELS contains all 5 blender:* IPC channels', () => {
    const preloadSource = readFileSync('electron/preload.ts', 'utf8');
    expect(preloadSource).toContain("'blender:status'");
    expect(preloadSource).toContain("'blender:detect'");
    expect(preloadSource).toContain("'blender:run-task'");
    expect(preloadSource).toContain("'blender:cancel-task'");
    expect(preloadSource).toContain("'blender:history'");
    expect(preloadSource).toContain('onBlenderEvent');
  });
});
