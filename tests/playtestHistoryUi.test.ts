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
    const load = app.slice(loadStart, app.indexOf('useEffect(() => {', loadStart));
    expect(load).toContain('const request = ++playtestHistoryRequest.current;');
    expect(load).toContain('if (!root) {\n      setPlaytestHistory([]);\n      setPlaytestHistoryError(null);');
    expect(load.match(/request === playtestHistoryRequest\.current/g)).toHaveLength(2);
    expect(load).toContain('setPlaytestHistory(entries);\n        setPlaytestHistoryError(null);');
    expect(load).toContain("setPlaytestHistoryError('Could not load playtest history.')");
    expect(load).not.toContain('sendDraft(');

    const refreshStart = app.indexOf('async function refreshWorkspaceState');
    const refresh = app.slice(refreshStart, app.indexOf('async function toggleDir', refreshStart));
    expect(refresh).toContain('const root = await api.projectRoot();');
    expect(refresh).toContain('await Promise.all([loadPlaytestHistory(root), loadRepairPlan(root)]);');
    expect(refresh.indexOf('const root = await api.projectRoot();')).toBeLessThan(refresh.indexOf('await Promise.all([loadPlaytestHistory(root), loadRepairPlan(root)]);'));
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
    expect(app.slice(refreshAt - 120, refreshAt)).toContain('onClick={() => void refreshPlaytestPanels()}');
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
    expect(app.match(/sendDraft\(/g)).toHaveLength(7);
    expect(app).toContain('const plan = await api.playtestRepairPlan().catch(() => null);');
    expect(app).toContain('await sendDraft(followupText(next, undefined, plan));');
    expect(app).toContain('needsRepairNudge(plan, next, turn.result.toolTrace)');
    expect(app).toContain('setRepairNudge({ plan, sessionId: turn.sessionId });');
    expect(app).toContain('const current = await api.playtestRepairPlan().catch(() => null);');
    expect(app).toContain('setRepairWriteNudge({ plan: current, definition, sessionId: first.sessionId });');
    expect(app).toContain('nudgeAction(repairWriteNudge, activeSession)');
    expect(app).toContain('nudgeAction(repairNudge, activeSession)');
    expect(app).toContain('void sendDraft(repairWriteNudgeText(plan, definition));');
    expect(app).toContain('const definition = await api.acceptanceChecks().catch(() => null);');
    expect(app).toContain('await sendDraft(repairNudgeText(plan, definition));');
    const repair = app.slice(app.indexOf('function repairFromLedger'), app.indexOf('async function refreshWorkspaceState'));
    expect(repair).not.toContain('sendDraft(');
  });

  it('loads the open repair plan and executes it with one click', () => {
    expect(app).toContain('api.playtestRepairPlan()');
    expect(app).toContain('setRepairPlan');
    expect(app).toMatch(/void api\.playtestRepairPlan\(\)[\s\S]{0,500}\}, \[projectRoot\]\);/);
    expect(app).toContain('<span>Open repair</span>');
    expect(app).toContain('Run repair');
    expect(app).toContain('repairPlan.scenario');
    expect(app).toContain('repairPlan.attempt');
    expect(app).toContain('repairPlan.failingCheck');
    expect(app).toContain('repairPlan.reason');
    expect(app).toContain('disabled={sending} onClick={() => void runRepair(repairPlan)}');
    const planEffectStart = app.indexOf('const request = ++repairPlanRequest.current;');
    const planEffect = app.slice(planEffectStart, planEffectStart + 900);
    expect(planEffect).toContain('void api.playtestRepairPlan()');
    expect(planEffect).toContain('if (!cancelled && request === repairPlanRequest.current)');
    const run = app.slice(app.indexOf('async function runRepair'), app.indexOf('async function refreshWorkspaceState'));
    expect(run).toContain('await sendDraft(acceptanceFirst(plan.instruction), { newSession: true, projectRoot })');
    expect(run).toContain("setScreen('chat')");
    expect(run).toContain('await loadRepairPlan()');
    expect(run).not.toContain('resolveApproval');
    expect(run).not.toContain("'approve'");
    const send = app.slice(app.indexOf('async function sendDraft'), app.indexOf('async function stopTurn'));
    expect(send).toContain('void loadRepairPlan();');
    expect(send).toContain('void loadPlaytestHistory();');
    const panels = app.slice(app.indexOf('async function refreshPlaytestPanels'), app.indexOf('async function runRepair'));
    expect(panels).toContain('loadPlaytestHistory()');
    expect(panels).toContain('loadRepairPlan()');
  });

  it('uses the failing check first and falls back to the reason', () => {
    const repair = app.slice(app.indexOf('function repairFromLedger'), app.indexOf('async function refreshWorkspaceState'));
    expect(repair).toContain('entry.failingCheck ?? entry.reason');
    expect(repair).toContain('setDraft(');
    expect(repair).toContain("setScreen('chat')");
  });
});
