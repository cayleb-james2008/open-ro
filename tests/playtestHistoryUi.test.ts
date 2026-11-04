import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');

describe('playtest history panel', () => {
  it('loads the ledger when a project opens and clears it otherwise', () => {
    expect(app).toContain('api.playtestLedger()');
    expect(app).toContain('setPlaytestHistory');
    expect(app).toMatch(/setPlaytestHistory\(\[\]\);\n    setPlaytestHistoryError\(null\);\n    if \(!projectRoot\)/);
    expect(app).toMatch(/if \(!projectRoot\) \{\s*return \(\) => \{/);

    const ledgerCall = app.indexOf('api.playtestLedger()');
    const effect = app.slice(app.lastIndexOf('useEffect(() => {', ledgerCall), app.indexOf('}, [projectRoot]);', ledgerCall));
    expect(effect).toContain('const request = ++playtestHistoryRequest.current;');
    expect(effect.match(/if \(!cancelled && request === playtestHistoryRequest\.current\)/g)).toHaveLength(2);
    expect(effect).toContain('cancelled = true');
    expect(effect).toContain("setPlaytestHistoryError('Could not load playtest history.')");
    expect(effect).not.toContain('setDraft(');
    expect(effect).not.toContain('sendDraft(');

    const loadStart = app.indexOf('async function loadPlaytestHistory(root: string | null = projectRoot)');
    const load = app.slice(loadStart, app.indexOf('function repairFromLedger', loadStart));
    expect(load).toContain('const request = ++playtestHistoryRequest.current;');
    expect(load).toContain('if (!root) {\n      setPlaytestHistory([]);\n      setPlaytestHistoryError(null);');
    expect(load.match(/request === playtestHistoryRequest\.current/g)).toHaveLength(2);
    expect(load).toContain('setPlaytestHistory(entries);\n        setPlaytestHistoryError(null);');
    expect(load).toContain("setPlaytestHistoryError('Could not load playtest history.')");
    expect(load).not.toContain('sendDraft(');

    const refreshStart = app.indexOf('async function refreshWorkspaceState');
    const refresh = app.slice(refreshStart, app.indexOf('async function toggleDir', refreshStart));
    expect(refresh).toContain('const root = await api.projectRoot();');
    expect(refresh).toContain('await loadPlaytestHistory(root);');
    expect(refresh.indexOf('const root = await api.projectRoot();')).toBeLessThan(refresh.indexOf('await loadPlaytestHistory(root);'));
  });

  it('renders a bounded history with verdict details and always-available refresh', () => {
    const panelStart = app.indexOf('<span>Playtest history</span>');
    expect(panelStart).toBeGreaterThan(-1);
    const listStart = app.indexOf('<div className="integration-list">', panelStart);
    const checkpointsAt = app.indexOf('<span>Checkpoints</span>', panelStart);
    expect(listStart).toBeGreaterThan(-1);
    expect(checkpointsAt).toBeGreaterThan(listStart);
    const panel = app.slice(panelStart, checkpointsAt);
    const refreshAt = app.indexOf('Refresh history', panelStart);
    expect(refreshAt).toBeGreaterThan(-1);
    expect(refreshAt).toBeLessThan(listStart);
    expect(app.slice(refreshAt - 120, refreshAt)).toContain('onClick={() => void loadPlaytestHistory()}');
    expect(panel).toContain('playtestHistory.slice(0, 10)');
    expect(panel).toContain('{entry.status} · {entry.scenario}');
    expect(panel).toContain("{entry.screenshots} screenshot(s) · {new Date(entry.createdAt).toLocaleString()} · {entry.file}");
    expect(panel).toContain('{entry.failingCheck ?? entry.reason}');
    expect(panel).toContain('{playtestHistoryError ? (');
    expect(panel).toContain('<b>History unavailable</b>');
    expect(panel).toContain('<p>{playtestHistoryError}</p>');
    expect(panel.indexOf('{playtestHistoryError ? (')).toBeLessThan(panel.indexOf('playtestHistory.length === 0 ? ('));
    expect(panel).not.toContain('{playtestHistoryError &&');
    expect(panel).not.toContain('sendDraft(');
  });

  it('offers Draft repair only for failed runs and never auto-sends', () => {
    expect(app).toContain("entry.status === 'FAILED'");
    expect(app).toContain('Draft repair');
    expect(app).toContain('function repairFromLedger(entry: PlaytestLedgerEntry)');
    expect(app.match(/repairFromLedger/g)).toHaveLength(2);
    expect(app).toContain('`Repair the last failed playtest for "${entry.scenario}". Failing check: ${failure}.');
    expect(app).toContain('Do not claim the playtest passes until report_playtest returns PASSED.');
    expect(app.match(/sendDraft\(/g)).toHaveLength(4);
    expect(app).toContain('void sendDraft(followupText(next));');
    const repair = app.slice(app.indexOf('function repairFromLedger'), app.indexOf('async function refreshWorkspaceState'));
    expect(repair).not.toContain('sendDraft(');
  });

  it('uses the failing check first and falls back to the reason', () => {
    const repair = app.slice(app.indexOf('function repairFromLedger'), app.indexOf('async function refreshWorkspaceState'));
    expect(repair).toContain('entry.failingCheck ?? entry.reason');
    expect(repair).toContain('setDraft(');
    expect(repair).toContain("setScreen('chat')");
  });
});
