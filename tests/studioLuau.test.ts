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

/** Decode a quoted Luau string that uses only \\ddd escapes, as luauString emits for \r and NUL. */
function decodeQuoted(literal: string): string {
  expect(literal).toMatch(/^"(?:[^"\\\0-\x1f\x7f]|\\\d{3})*"$/);
  return literal.slice(1, -1).replace(/\\(\d{3})/g, (_, code: string) => String.fromCharCode(Number(code)));
}

describe('Luau literals', () => {
  it('values with \\r or NUL become quoted strings that decode exactly', () => {
    for (const value of ['print(1)\r\nprint(2)\r\n', '\n\rx', '\r', 'a\0b', '\0', '"q" \\ ]] \t\x7f é\r', '\\013\r']) {
      const literal = luauString(value);
      expect(literal).not.toMatch(/[\r\n\0]/);
      expect(decodeQuoted(literal)).toBe(value);
    }
  });

  it('a CRLF write keeps the line endings it was reviewed with', () => {
    const luau = buildWriteLuau({ segments: ['ServerScriptService', 'Shop'], className: 'Script', source: 'print(1)\r\nprint(2)\r\n', expectedPrevious: 'print(0)\r\n' });
    expect(luau).not.toMatch(/[\r\0]/);
    expect(luau).toContain(`, ${luauString('print(1)\r\nprint(2)\r\n')}\n`);
    expect(luau).toContain('local expectedPrevious = "print(0)\\013\\010"');
  });

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
