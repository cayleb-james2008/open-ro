import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import { buildCss } from '../scripts/build-tokens.mjs';

const tokens = JSON.parse(readFileSync('design/tokens.json', 'utf8'));
const hex = (role: string): string => tokens.palette[tokens.color[role]];
const luminance = (value: string): number => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(value.slice(i, i + 2), 16) / 255)
    .map((c) => (c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const contrast = (a: string, b: string): number => {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
};

describe('design tokens', () => {
  it('generated tokens.css matches tokens.json', () => {
    expect(readFileSync('src/tokens.css', 'utf8')).toBe(buildCss(tokens));
  });

  it('uses one accent, taken from the Catppuccin Mocha accents', () => {
    expect(tokens.color.accent).toBe('lavender');
    expect(tokens.palette.lavender).toBe('#b4befe');
    expect(tokens.palette.base).toBe('#1e1e2e');
  });

  it('meets WCAG AA for body text (4.5:1) on every surface', () => {
    for (const fg of ['text', 'text-2', 'muted', 'dim', 'accent', 'danger', 'warning', 'success']) {
      for (const bg of ['bg', 'sidebar', 'surface', 'raised']) {
        expect(contrast(hex(fg), hex(bg)), `${fg} on ${bg}`).toBeGreaterThanOrEqual(4.5);
      }
    }
    expect(contrast(hex('on-accent'), hex('accent')), 'on-accent on accent').toBeGreaterThanOrEqual(4.5);
  });

  it('runs the token check as part of npm run lint', () => {
    const scripts = JSON.parse(readFileSync('package.json', 'utf8')).scripts;
    expect(scripts.lint).toContain('node scripts/build-tokens.mjs --check');
  });
});
