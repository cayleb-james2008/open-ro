import { describe, expect, it } from 'vitest';
import { DIFF_PREVIEW_ROWS, diffPreview } from '../src/textDiff';

describe('approval diff preview', () => {
  const lines = (count: number, tag: string) => Array.from({ length: count }, (_, index) => `${tag}${index}`).join('\n');

  it('says how many rows are cut until the diff is expanded', () => {
    const cut = diffPreview('', lines(300, 'x'), false);
    expect(cut.rows).toHaveLength(DIFF_PREVIEW_ROWS);
    expect(DIFF_PREVIEW_ROWS).toBe(240);
    expect(cut.hidden).toBe(60);
    const whole = diffPreview('', lines(300, 'x'), true);
    expect(whole.rows).toHaveLength(300);
    expect(whole.hidden).toBe(0);
    expect(diffPreview('', lines(240, 'x'), false).hidden).toBe(0);
    expect(diffPreview('', lines(241, 'x'), false).hidden).toBe(1);
  });

  it('flags a change that no row shows, such as LF to CRLF', () => {
    expect(diffPreview('a\nb\n', 'a\r\nb\r\n', false)).toMatchObject({ hidden: 0, invisibleChange: true });
    expect(diffPreview('a\nb', 'a\nb', false).invisibleChange).toBe(false);
    expect(diffPreview('a', 'b', false).invisibleChange).toBe(false);
  });
});
