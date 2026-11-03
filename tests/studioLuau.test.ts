import { describe, expect, it } from 'vitest';
import { buildWriteLuau, luauString } from '../electron/studioLuau';

/** Decode a Luau long string the way the Luau lexer does: up to the first matching close bracket. */
function decodeLongString(literal: string): { value: string; rest: string } {
  const open = /^\[(=*)\[/.exec(literal);
  expect(open).not.toBeNull();
  const eq = open?.[1] ?? '';
  const close = `]${eq}]`;
  let body = literal.slice(eq.length + 2);
  if (body.startsWith('\r\n')) {
    body = body.slice(2);
  } else if (body.startsWith('\n') || body.startsWith('\r')) {
    body = body.slice(1);
  }
  const end = body.indexOf(close);
  return { value: body.slice(0, end), rest: body.slice(end + close.length) };
}

describe('Luau literals', () => {
  it('long strings survive their own closing brackets and a leading newline', () => {
    for (const value of ['plain', 'a]]b', 'x]=]y]]', 'ends]', 'ends]=', '\nstarts with newline', '', ']==]]=]]]']) {
      const literal = luauString(value);
      expect(decodeLongString(literal)).toEqual({ value, rest: '' });
    }
  });

  it('a write embeds the source as data, never as code', () => {
    const source = ']]) os.exit() --[[';
    const luau = buildWriteLuau({ segments: ['ServerScriptService', 'Shop'], className: 'Script', source, expectedPrevious: null });
    const line = luau.split('\n').find((text) => text.startsWith('local name, className, source')) ?? '';
    const literal = line.slice(line.indexOf(luauString(source)));
    expect(literal).toBe(luauString(source));
    expect(decodeLongString(literal)).toEqual({ value: source, rest: '' });
    expect(luau).toContain('local expectedPrevious = nil');
  });
});
