import { describe, expect, it } from 'vitest';
import { currencyProblems, normalizeSnapshot, parseInstancePath, parseLuauReply, placementProblem, readbackProblems } from '../electron/studioChecks';

describe('Studio instance paths', () => {
  it('accepts game-prefixed and bare dot paths and refuses empty segments', () => {
    expect(parseInstancePath('game.ServerScriptService.Shop')).toEqual(['ServerScriptService', 'Shop']);
    expect(parseInstancePath(' StarterGui.HUD ')).toEqual(['StarterGui', 'HUD']);
    expect(() => parseInstancePath('game..Shop')).toThrow(/usable instance path/);
    expect(() => parseInstancePath('game')).toThrow(/usable instance path/);
    expect(() => parseInstancePath('Workspace.Bad\nName')).toThrow(/usable instance path/);
  });

  it('refuses scripts placed where they cannot run', () => {
    expect(placementProblem(['ServerScriptService', 'Shop'], 'Script')).toBeNull();
    expect(placementProblem(['StarterGui', 'HUD', 'Client'], 'LocalScript')).toBeNull();
    expect(placementProblem(['StarterGui', 'Shop'], 'Script')).toMatch(/does not run in StarterGui/);
    expect(placementProblem(['ServerScriptService', 'Ui'], 'LocalScript')).toMatch(/does not run in ServerScriptService/);
    expect(placementProblem(['ServerScriptService'], 'Script')).toMatch(/needs a name under a service/);
  });
});

describe('server-owned currency checks', () => {
  const server = ['ServerScriptService', 'Shop'];
  it('blocks client code that changes currency', () => {
    expect(currencyProblems(['StarterGui', 'HUD', 'Buy'], 'LocalScript', 'player.leaderstats.Coins.Value += 10')[0]).toMatch(/server must own/);
    expect(currencyProblems(['ReplicatedFirst', 'M'], 'ModuleScript', 'stats.Gems.Value = 0')[0]).toMatch(/server must own/);
  });
  it('requires --!strict and typeof checks on server currency handlers', () => {
    const loose = 'Remote.OnServerEvent:Connect(function(p, amount)\n  p.leaderstats.Coins.Value += amount\nend)';
    expect(currencyProblems(server, 'Script', loose)).toEqual([
      expect.stringMatching(/--!strict/),
      expect.stringMatching(/typeof/),
    ]);
    const checked = `--!strict\nRemote.OnServerEvent:Connect(function(p, amount: unknown)\n  if typeof(amount) ~= "number" then return end\n  p.leaderstats.Coins.Value += 5\nend)`;
    expect(currencyProblems(server, 'Script', checked)).toEqual([]);
  });
  it('ignores reads, comparisons, comments and unrelated values', () => {
    expect(currencyProblems(server, 'Script', 'if stats.Coins.Value == 5 then print(stats.Coins.Value) end')).toEqual([]);
    expect(currencyProblems(['StarterGui', 'X'], 'LocalScript', '-- leaderstats.Coins.Value += 1\nlabel.Text = coins.Value')).toEqual([]);
    expect(currencyProblems(['StarterGui', 'X'], 'LocalScript', 'health.Value -= 1')).toEqual([]);
  });
});

describe('Studio replies', () => {
  it('reads the marked reply raw or JSON-quoted, and fails closed otherwise', () => {
    const raw = 'noise\nOPENRO:{"ok":true,"result":{"exists":false,"missing":"Teams"}}\nmore';
    expect(parseLuauReply(raw)).toEqual({ ok: true, result: { exists: false, missing: 'Teams' } });
    expect(parseLuauReply(JSON.stringify(raw))).toEqual({ ok: true, result: { exists: false, missing: 'Teams' } });
    expect(parseLuauReply('OPENRO:{"ok":false,"error":"Missing parent X"}')).toEqual({ ok: false, error: 'Missing parent X' });
    expect(parseLuauReply('Error: execute_luau is disabled')).toEqual({ ok: false, error: expect.stringMatching(/did not return Open-Ro's reply/) });
    expect(parseLuauReply('OPENRO:{"ok":true,"result":[1]}').ok).toBe(false);
    expect(parseLuauReply('OPENRO:not json').ok).toBe(false);
  });

  it('a snapshot keeps only what Studio reported for requested services', () => {
    const snapshot = normalizeSnapshot({
      truncated: false,
      services: [
        { name: 'Workspace', present: true, nodes: [{ path: 'Workspace.Baseplate', className: 'Part', depth: 1 }, { path: 7, className: 'Part', depth: 1 }] },
        { name: 'Teams', present: false, nodes: [{ path: 'Teams.Red', className: 'Team', depth: 1 }] },
        { name: 'Unrequested', present: true, nodes: [{ path: 'Unrequested.X', className: 'Folder', depth: 1 }] },
        { name: 'Lighting', present: 'yes', nodes: [] },
      ],
    }, ['Workspace', 'Teams', 'Lighting', 'StarterGui']);
    expect(snapshot.services.map((s) => [s.name, s.present, s.nodes.length])).toEqual([
      ['Workspace', true, 1], ['Teams', false, 0], ['Lighting', false, 0], ['StarterGui', false, 0],
    ]);
    expect(snapshot.missing).toEqual(['Teams', 'Lighting', 'StarterGui']);
    expect(snapshot.nodeCount).toBe(1);
    expect(snapshot.truncated).toBe(false);
  });

  it('a read-back must match the reviewed path, class and source', () => {
    const ok = { path: 'ServerScriptService.Shop', className: 'Script', source: 'print(1)' };
    expect(readbackProblems(['ServerScriptService', 'Shop'], 'Script', 'print(1)', ok)).toEqual([]);
    expect(readbackProblems(['ServerScriptService', 'Shop'], 'Script', 'print(2)', ok)).toEqual(['Read-back source does not match the approved source.']);
    expect(readbackProblems(['ServerScriptService', 'Shop2'], 'LocalScript', 'print(1)', ok)).toHaveLength(2);
    expect(readbackProblems(['ServerScriptService', 'Shop'], 'Script', 'print(1)', {})).toHaveLength(3);
  });
});
