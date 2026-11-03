import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const read = (file: string) => readFileSync(new URL(`../${file}`, import.meta.url), 'utf8');

describe('dev-only comps', () => {
  it('keeps comps styles out of the production stylesheet and loads comps only in dev', () => {
    expect(read('src/styles.css')).not.toContain('comps-');
    expect(read('src/comps/comps.css')).toContain('.comps-index');
    expect(read('src/comps/Comps.tsx')).toContain("import './comps.css';");
    const main = read('src/main.tsx');
    expect(main).not.toContain('comps.css');
    expect(main).toContain("if (import.meta.env.DEV && location.pathname.startsWith('/comps/')) {");
    expect(main.match(/import\(['"]\.\/comps\/Comps['"]\)/g)).toHaveLength(1);
    expect(main).not.toMatch(/^import .*comps/m);
  });
});
