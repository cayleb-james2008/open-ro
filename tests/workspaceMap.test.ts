import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

describe('workspace project map', () => {
  const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');

  it('loads the index for the open project', () => {
    expect(app).toContain('api.indexProject()');
    expect(app).toContain('setProjectIndex');
    expect(app).toMatch(/void api\.indexProject\(\)[\s\S]{0,700}\}, \[projectRoot\]\);/);
  });

  it('renders a bounded map with sizes, the 40-row cap, and truncation state', () => {
    expect(app).toContain('<span>Project map</span>');
    expect(app).toContain('entries.slice(0, 40)');
    expect(app).toContain('bytes');
    expect(app).toContain('The index hit a cap; large folders are partially listed.');
    expect(app).toContain('Showing 40 of');
  });

  it('opens an indexed file from the map', () => {
    expect(app).toContain('onClick={() => void openFile(entry.relPath)}');
  });

  it('clears the map when no project is open', () => {
    expect(app).toContain('if (!projectRoot) {');
    expect(app).toContain('setProjectIndex(null)');
  });
});
