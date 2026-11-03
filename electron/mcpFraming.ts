/**
 * MCP stdio framing. Servers send either LSP-style Content-Length headers
 * or newline-delimited JSON. Both show up in the wild; a client that only
 * splits on newlines cannot talk to a header-framed Studio server.
 */

export interface McpParseResult {
  buffer: string;
  messages: unknown[];
  errors: string[];
}

function headerBreak(buffer: string): { index: number; length: number } | null {
  const crlf = buffer.indexOf('\r\n\r\n');
  const lf = buffer.indexOf('\n\n');
  if (crlf < 0 && lf < 0) {
    return null;
  }
  if (crlf < 0) {
    return { index: lf, length: 2 };
  }
  if (lf < 0) {
    return { index: crlf, length: 4 };
  }
  return crlf <= lf ? { index: crlf, length: 4 } : { index: lf, length: 2 };
}

export function pushMcpChunk(buffer: string, chunk: string): McpParseResult {
  let pending = buffer + chunk;
  const messages: unknown[] = [];
  const errors: string[] = [];

  for (;;) {
    pending = pending.replace(/^\r?\n/, '');
    if (pending.length === 0) {
      break;
    }
    if (/^content-length:/i.test(pending)) {
      const header = headerBreak(pending);
      if (!header) {
        break;
      }
      const head = pending.slice(0, header.index);
      const match = /content-length:\s*(\d+)/i.exec(head);
      if (!match) {
        errors.push('MCP frame is missing Content-Length.');
        pending = pending.slice(header.index + header.length);
        continue;
      }
      const length = Number(match[1]);
      const start = header.index + header.length;
      if (pending.length < start + length) {
        break;
      }
      const body = pending.slice(start, start + length);
      pending = pending.slice(start + length);
      try {
        messages.push(JSON.parse(body) as unknown);
      } catch {
        errors.push('MCP frame body was not valid JSON.');
      }
      continue;
    }

    const newline = pending.indexOf('\n');
    if (newline < 0) {
      break;
    }
    const line = pending.slice(0, newline).trim();
    pending = pending.slice(newline + 1);
    if (!line) {
      continue;
    }
    try {
      messages.push(JSON.parse(line) as unknown);
    } catch {
      errors.push('MCP line was not valid JSON.');
    }
  }

  return { buffer: pending, messages, errors };
}

export function assertMcpSpec(spec: { command?: unknown; args?: unknown }): { command: string; args: string[] } {
  if (typeof spec.command !== 'string' || spec.command.trim() === '') {
    throw new Error('MCP command is required.');
  }
  if (spec.command.includes('\0')) {
    throw new Error('Invalid MCP command.');
  }
  const args = spec.args ?? [];
  if (!Array.isArray(args) || args.some((item) => typeof item !== 'string')) {
    throw new Error('MCP arguments must be a list of strings.');
  }
  return { command: spec.command.trim(), args };
}
