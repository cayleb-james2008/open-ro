/**
 * Playtest ledger summaries. Pure module: the main process reads record files
 * and passes their text here; the renderer can type against the result without
 * pulling Node builtins in.
 */

export type PlaytestLedgerStatus = 'PASSED' | 'FAILED' | 'NO_EVIDENCE';

export interface PlaytestLedgerRecord {
  file: string;
  modifiedMs: number;
  raw: string;
}

export interface PlaytestLedgerEntry {
  file: string;
  scenario: string;
  status: PlaytestLedgerStatus;
  reason: string;
  failingCheck?: string;
  screenshots: number;
  createdAt: string;
}

const MAX_ENTRIES = 50;

function normalizeRoot(value: string): string {
  const trimmed = value.trim();
  const normalized = trimmed.replace(/\\/g, '/').replace(/\/+$/, '');
  const windowsStyle = trimmed.includes('\\') || /^[a-z]:\//i.test(normalized);
  return windowsStyle ? normalized.toLowerCase() : normalized;
}

function statusOf(value: unknown): PlaytestLedgerStatus | null {
  return value === 'PASSED' || value === 'FAILED' || value === 'NO_EVIDENCE' ? value : null;
}

/**
 * Summarize stored playtest records for one project. Malformed records and
 * records from other projects are skipped; the newest entries come first.
 */
export function summarizePlaytestLedger(records: PlaytestLedgerRecord[], projectRoot: string, limit = MAX_ENTRIES): PlaytestLedgerEntry[] {
  const wanted = normalizeRoot(projectRoot);
  const entries: PlaytestLedgerEntry[] = [];
  for (const record of records) {
    let parsed: unknown;
    try {
      parsed = JSON.parse(record.raw);
    } catch {
      continue;
    }
    if (!parsed || typeof parsed !== 'object') {
      continue;
    }
    const stored = parsed as {
      projectRoot?: unknown;
      report?: { scenario?: unknown };
      verdict?: { status?: unknown; reason?: unknown; failingCheck?: unknown; screenshots?: unknown };
    };
    if (typeof stored.projectRoot !== 'string' || normalizeRoot(stored.projectRoot) !== wanted) {
      continue;
    }
    const verdict = stored.verdict;
    if (!verdict) {
      continue;
    }
    const status = statusOf(verdict.status);
    if (!status) {
      continue;
    }
    const scenario = typeof stored.report?.scenario === 'string' ? stored.report.scenario.slice(0, 200) : '(no scenario)';
    const reason = typeof verdict.reason === 'string' ? verdict.reason.slice(0, 400) : '';
    const failingCheck = typeof verdict.failingCheck === 'string' ? verdict.failingCheck.slice(0, 400) : undefined;
    const screenshots = Array.isArray(verdict.screenshots) ? verdict.screenshots.length : 0;
    const modified = Number.isFinite(record.modifiedMs) ? record.modifiedMs : 0;
    entries.push({
      file: record.file,
      scenario,
      status,
      reason,
      ...(failingCheck ? { failingCheck } : {}),
      screenshots,
      createdAt: new Date(modified).toISOString(),
    });
  }
  return entries
    .sort((left, right) => (right.createdAt === left.createdAt ? left.file.localeCompare(right.file) : right.createdAt.localeCompare(left.createdAt)))
    .slice(0, limit);
}
