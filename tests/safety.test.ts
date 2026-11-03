import { mkdtemp, mkdir, realpath, rm, symlink, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../electron/approvals';
import { assertMcpSpec, pushMcpChunk } from '../electron/mcpFraming';
import { isInsideRoot, resolveWithinRoot } from '../electron/pathSafety';
import { activateProvider, listProviderViews, parseProviderState, rememberProjectRoot, removeProvider, upsertProvider, validateProviderSave } from '../electron/providerState';
import { getProviderOption } from '../electron/providerCatalog';
import { runShellCommand, shellInvocation } from '../electron/runCommand';
import { sanitizeSessions } from '../electron/sessions';
import { scrubbedEnv } from '../electron/toolEnv';
import { diffLines } from '../src/textDiff';

const tempDirs: string[] = [];

afterEach(async () => {
  await Promise.all(tempDirs.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
});

async function tempDir(prefix: string): Promise<string> {
  const dir = await mkdtemp(path.join(tmpdir(), prefix));
  tempDirs.push(dir);
  return dir;
}

describe('project path safety', () => {
  it('rejects prefix siblings, parent traversal, symlinks, secrets, and ignored folders', async () => {
    const parent = await tempDir('openro-parent-');
    const root = path.join(parent, 'proj');
    const evil = path.join(parent, 'proj-evil');
    const outside = await tempDir('openro-outside-');
    await mkdir(root);
    await mkdir(evil);
    await writeFile(path.join(root, 'ok.txt'), 'yes');
    await writeFile(path.join(evil, 'x.txt'), 'no');
    await writeFile(path.join(outside, 'secret.txt'), 'nope');
    await symlink(outside, path.join(root, 'link'));

    const ok = await resolveWithinRoot(root, 'ok.txt');
    expect(ok).toBe(await realpath(path.join(root, 'ok.txt')));
    await expect(resolveWithinRoot(root, '../proj-evil/x.txt')).rejects.toThrow(/escapes/);
    await expect(resolveWithinRoot(root, 'link/secret.txt')).rejects.toThrow(/escapes/);
    await expect(resolveWithinRoot(root, '.env')).rejects.toThrow(/secret/);
    await expect(resolveWithinRoot(root, 'node_modules/leftpad/index.js')).rejects.toThrow(/ignored/);
    const created = await resolveWithinRoot(root, 'src/new/file.lua');
    expect(created.startsWith(await realpath(root) + path.sep)).toBe(true);
    expect(isInsideRoot(root, path.join(root, 'src'))).toBe(true);
    expect(isInsideRoot(root, evil)).toBe(false);
  });
});

describe('approval queue', () => {
  it('returns a staged payload once and drops it after it expires', () => {
    let now = 1_000;
    const queue = new ApprovalQueue(() => now, 1_000);
    const write = queue.stageWrite({ projectRoot: '/proj', relPath: 'a.txt', content: 'next', previous: 'prev' });
    const command = queue.stageCommand({ projectRoot: '/proj', command: 'rojo build', cwd: '/proj' });
    expect(queue.list().map((item) => item.id)).toEqual([write.id, command.id]);
    expect(queue.take(write.id).content).toBe('next');
    expect(() => queue.take(write.id)).toThrow(/no longer pending/);
    now = 3_000;
    expect(queue.list()).toEqual([]);
    expect(() => queue.take(command.id)).toThrow(/no longer pending/);
  });
});

describe('provider state', () => {
  it('keeps every saved model, shows keyless providers, and clears the active provider on delete', () => {
    const legacy = parseProviderState({ active: { kind: 'ollama', model: 'qwen2.5-coder', baseUrl: 'http://127.0.0.1:11434/v1' } });
    expect(legacy.activeKind).toBe('ollama');
    const saved = upsertProvider(legacy, { kind: 'openai-api', model: 'gpt-5.2' });
    expect(saved.activeKind).toBe('openai-api');
    expect(saved.providers).toHaveLength(2);
    const views = listProviderViews(saved, { credentialServices: ['provider:openai-api'], secure: true });
    expect(views.find((view) => view.kind === 'ollama')).toMatchObject({ hasKey: true, active: false, model: 'qwen2.5-coder' });
    expect(views.find((view) => view.kind === 'openai-api')).toMatchObject({ hasKey: true, active: true, model: 'gpt-5.2' });
    const removed = removeProvider(saved, 'openai-api');
    expect(removed.activeKind).toBeUndefined();
    expect(() => activateProvider(removed, 'openai-api')).toThrow(/Save this provider/);
  });

  it('requires an API key only for providers that bill through a key', () => {
    expect(() => validateProviderSave(getProviderOption('openai-api'), { model: 'gpt-5.2', existingKey: false })).toThrow(/API key/);
    expect(validateProviderSave(getProviderOption('openai-api'), { model: 'gpt-5.2', apiKey: 'sk-test', existingKey: false }).apiKey).toBe('sk-test');
    expect(validateProviderSave(getProviderOption('custom-openai'), { model: 'local', baseUrl: 'http://127.0.0.1:8000/v1', existingKey: false }).apiKey).toBeUndefined();
    expect(() => validateProviderSave(getProviderOption('custom-openai'), { model: 'local', existingKey: false })).toThrow(/base URL/);
    expect(validateProviderSave(getProviderOption('codex-cli'), { model: '', existingKey: false }).model).toBe('');
    expect(() => validateProviderSave(getProviderOption('ollama'), { model: '  ', existingKey: false })).toThrow(/model ID/);
  });

  it('restores a saved project folder and does not invent the working directory', () => {
    const saved = rememberProjectRoot({ providers: [] }, '  /proj/game  ');
    const roundTrip = parseProviderState(JSON.parse(JSON.stringify(saved)));
    expect(roundTrip.projectRoot).toBe('/proj/game');
    expect(parseProviderState({ projectRoot: '' }).projectRoot).toBeUndefined();
    expect(parseProviderState({ projectRoot: '   ' }).projectRoot).toBeUndefined();
    expect(parseProviderState({ projectRoot: '.' }).projectRoot).toBeUndefined();
    expect(parseProviderState({ providers: [] }).projectRoot).toBeUndefined();
    expect(upsertProvider(saved, { kind: 'ollama', model: 'qwen2.5-coder' }).projectRoot).toBe('/proj/game');
    expect(removeProvider(saved, 'ollama').projectRoot).toBe('/proj/game');
  });
});

describe('mcp framing', () => {
  it('reassembles Content-Length frames and newline JSON', () => {
    const body = JSON.stringify({ jsonrpc: '2.0', id: 1, result: { ok: true } });
    const frame = `Content-Length: ${Buffer.byteLength(body, 'utf8')}\r\n\r\n${body}`;
    const first = pushMcpChunk('', frame.slice(0, 12));
    expect(first.messages).toEqual([]);
    const second = pushMcpChunk(first.buffer, frame.slice(12));
    expect(second.messages).toEqual([{ jsonrpc: '2.0', id: 1, result: { ok: true } }]);
    const lines = pushMcpChunk('', '{"jsonrpc":"2.0","id":2,"result":{}}\n{"nope"\n');
    expect(lines.messages).toHaveLength(1);
    expect(lines.errors).toEqual(['MCP line was not valid JSON.']);
    expect(assertMcpSpec({ command: ' StudioMCP ', args: ['--stdio'] })).toEqual({ command: 'StudioMCP', args: ['--stdio'] });
    expect(() => assertMcpSpec({ command: '', args: [] })).toThrow(/required/);
    expect(() => assertMcpSpec({ command: 'mcp', args: [1] as unknown as string[] })).toThrow(/list of strings/);
  });
});

describe('command execution', () => {
  it('plans a shell without taking a renderer-supplied working directory', () => {
    expect(shellInvocation('rojo build', 'linux')).toEqual({ file: '/bin/sh', args: ['-lc', 'rojo build'] });
    expect(shellInvocation('rojo build', 'win32').args).toEqual(['/c', 'rojo build']);
  });

  it('runs an approved command without inheriting secret environment variables', async () => {
    const previous = process.env.OPENAI_API_KEY;
    process.env.OPENAI_API_KEY = 'SENTINEL_SHOULD_NOT_LEAK';
    try {
      expect(scrubbedEnv(process.env).OPENAI_API_KEY).toBeUndefined();
      const result = await runShellCommand({
        command: 'node -e "process.stdout.write(process.env.OPENAI_API_KEY || \'\')"',
        cwd: process.cwd(),
        timeoutMs: 10_000,
      });
      expect(result.timedOut).toBe(false);
      expect(result.code).toBe(0);
      expect(result.stdout).toBe('');
    } finally {
      if (previous === undefined) {
        delete process.env.OPENAI_API_KEY;
      } else {
        process.env.OPENAI_API_KEY = previous;
      }
    }
  });

  it('stops a command that exceeds its timeout', async () => {
    const result = await runShellCommand({ command: 'sleep 30', cwd: process.cwd(), timeoutMs: 1_000 });
    expect(result.timedOut).toBe(true);
  });
});

describe('text diff and session sanitizing', () => {
  it('diffs changed lines and does not treat an empty file as a blank line', () => {
    expect(diffLines('a\nb', 'a\nc')).toEqual([
      { kind: 'context', text: 'a' },
      { kind: 'del', text: 'b' },
      { kind: 'add', text: 'c' },
    ]);
    expect(diffLines('', 'hi')).toEqual([{ kind: 'add', text: 'hi' }]);
  });

  it('drops malformed sessions and fields that are not part of the chat record', () => {
    const snapshot = sanitizeSessions({
      activeId: 'chat-1',
      sessions: [
        {
          id: 'chat-1',
          title: 'Fix the door',
          apiKey: 'sk-nope',
          messages: [{ role: 'user', content: 'hello', apiKey: 'sk-nope', trace: [{ name: 'read_file', status: 'executed', detail: 'src/door.luau' }] }, { role: 'system', content: 'hidden' }],
        },
        { id: '../etc/passwd', title: 'no', messages: [] },
      ],
    });
    expect(snapshot.activeId).toBe('chat-1');
    expect(snapshot.sessions).toHaveLength(1);
    expect(JSON.stringify(snapshot)).not.toContain('sk-nope');
    expect(snapshot.sessions[0]?.messages).toEqual([
      { role: 'user', content: 'hello', trace: [{ name: 'read_file', status: 'executed', detail: 'src/door.luau' }] },
    ]);
  });
});
