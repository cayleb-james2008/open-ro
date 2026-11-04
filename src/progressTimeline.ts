/**
 * Plan & progress timeline. Pure module: derives one chronological view from
 * state the app already has (open repair plan, playtest ledger, project chats).
 * No new persistence; the renderer can import the types without Node builtins.
 */

import type { PlaytestLedgerEntry, PlaytestRepairPlan } from '../electron/playtestLedger';
import type { StoredChatSession } from '../electron/sessions';
import { isSessionVisibleForProject } from './sessionScope';

export type ProgressEventKind = 'repair' | 'playtest' | 'chat';

export interface ProgressEvent {
  id: string;
  kind: ProgressEventKind;
  title: string;
  detail: string;
  time: string;
  status: string;
}

export interface ProgressSummary {
  runs: number;
  passed: number;
  failed: number;
  openAttempt: number | null;
  scenario: string | null;
  chats: number;
}

export interface ProgressInput {
  repairPlan: PlaytestRepairPlan | null;
  playtestHistory: PlaytestLedgerEntry[];
  sessions: StoredChatSession[];
  projectRoot: string | null;
}

export function progressSummary(input: ProgressInput): ProgressSummary {
  const history = input.playtestHistory;
  const chats = input.sessions.filter((session) => isSessionVisibleForProject(session, input.projectRoot));
  return {
    runs: history.length,
    passed: history.filter((entry) => entry.status === 'PASSED').length,
    failed: history.filter((entry) => entry.status === 'FAILED').length,
    openAttempt: input.repairPlan?.attempt ?? null,
    scenario: input.repairPlan?.scenario ?? null,
    chats: chats.length,
  };
}

/** Newest first. Invalid timestamps sort last instead of throwing. */
export function buildProgressTimeline(input: ProgressInput, limit = 12): ProgressEvent[] {
  const events: ProgressEvent[] = [];
  if (input.repairPlan) {
    const plan = input.repairPlan;
    events.push({
      id: `repair-${plan.lastFailedFile}`,
      kind: 'repair',
      title: `Repair attempt ${plan.attempt} — ${plan.scenario}`,
      detail: plan.failingCheck,
      time: plan.lastFailedAt,
      status: 'OPEN',
    });
  }
  for (const entry of input.playtestHistory) {
    events.push({
      id: `playtest-${entry.file}`,
      kind: 'playtest',
      title: `${entry.status} — ${entry.scenario}`,
      detail: entry.failingCheck ?? entry.reason,
      time: entry.createdAt,
      status: entry.status,
    });
  }
  for (const session of input.sessions.filter((item) => isSessionVisibleForProject(item, input.projectRoot))) {
    events.push({
      id: `chat-${session.id}`,
      kind: 'chat',
      title: session.title || 'Untitled chat',
      detail: `${session.messages.length} message${session.messages.length === 1 ? '' : 's'}`,
      time: session.updatedAt,
      status: 'CHAT',
    });
  }
  return events
    .map((event) => ({ event, order: Number.isFinite(Date.parse(event.time)) ? Date.parse(event.time) : -Infinity }))
    .sort((a, b) => b.order - a.order)
    .slice(0, limit)
    .map(({ event }) => event);
}
