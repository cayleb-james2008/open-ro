import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
const styles = readFileSync('src/styles.css', 'utf8').replace(/\r\n/g, '\n');

describe('Blender 3D Asset Pipeline UI: App.tsx Component Contracts', () => {
  it('renders Blender 3D section in WorkspacePage with install status indicator', () => {
    expect(app).toContain('Blender 3D Asset Pipeline');
    expect(app).toContain('blenderStatus');
    expect(app).toContain('Re-detect');
  });

  it('renders 3D task selector with the 5 core operations', () => {
    expect(app).toContain('inspect_mesh');
    expect(app).toContain('convert_format');
    expect(app).toContain('decimate_mesh');
    expect(app).toContain('generate_collision');
    expect(app).toContain('export_roblox_package');
  });

  it('filters project file dropdown for 3D formats (.obj, .fbx, .gltf, .glb)', () => {
    expect(app).toContain('(obj|fbx|gltf|glb)');
  });

  it('renders decimation controls: triangle budget, ratio slider, UV preservation', () => {
    expect(app).toContain('blenderTargetTris');
    expect(app).toContain('blenderDecimateRatio');
    expect(app).toContain('Preserve UV Seams');
  });

  it('renders action buttons: Stage for Approval, Run 3D Task, and Cancel Execution', () => {
    expect(app).toContain('Stage for Approval');
    expect(app).toContain('Run 3D Task');
    expect(app).toContain('Cancel Execution');
  });
});

describe('Blender 3D Asset Pipeline UI: styles.css Stylesheet Rules', () => {
  it('defines stylesheet classes for blender-panel, progress bar, metrics, and logs', () => {
    expect(styles).toContain('.blender-panel');
    expect(styles).toContain('.blender-progress-banner');
    expect(styles).toContain('.metrics-grid');
    expect(styles).toContain('.blender-logs');
  });
});
