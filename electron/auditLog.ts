/**
 * Append-only security audit log for Open-Ro.
 * Tracks every approval lifecycle event, tool invocation, file write,
 * and command execution in a structured, redacted format.
 */

import { redactObject } from './redaction';

export type AuditCategory =
  | 'APPROVAL_STAGED'
  | 'APPROVAL_GRANTED'
  | 'APPROVAL_REJECTED'
  | 'APPROVAL_EXPIRED'
  | 'TOOL_INVOKED'
  | 'TOOL_RESULT'
  | 'FILE_MUTATION'
  | 'COMMAND_EXECUTION'
  | 'STUDIO_MUTATION';

export type AuditActor = 'user' | 'agent' | 'system';

export interface AuditEvent {
  id: string;
  timestamp: string;
  category: AuditCategory;
  actor: AuditActor;
  sessionId?: string;
  details: Record<string, unknown>;
}

const MAX_IN_MEMORY_EVENTS = 500;
const inMemoryAuditLog: AuditEvent[] = [];
let eventCounter = 0;

/**
 * Appends a new redacted security audit entry to the active audit log.
 */
export function logAuditEvent(entry: {
  category: AuditCategory;
  actor: AuditActor;
  sessionId?: string;
  details: Record<string, unknown>;
}): AuditEvent {
  eventCounter += 1;
  const event: AuditEvent = {
    id: `audit-${Date.now()}-${eventCounter}`,
    timestamp: new Date().toISOString(),
    category: entry.category,
    actor: entry.actor,
    sessionId: entry.sessionId,
    details: redactObject(entry.details),
  };

  inMemoryAuditLog.push(event);
  if (inMemoryAuditLog.length > MAX_IN_MEMORY_EVENTS) {
    inMemoryAuditLog.shift();
  }

  return event;
}

/**
 * Retrieves recent audit entries, optionally filtered by category and count limit.
 */
export function getRecentAuditEvents(limit = 100, category?: AuditCategory): AuditEvent[] {
  const filtered = category
    ? inMemoryAuditLog.filter((evt) => evt.category === category)
    : inMemoryAuditLog;
  return filtered.slice(-Math.max(1, limit));
}

/**
 * Returns total count of audit events recorded in current process lifetime.
 */
export function getAuditEventCount(): number {
  return inMemoryAuditLog.length;
}

/**
 * Serializes the current audit log to formatted JSON for export or inspection.
 */
export function exportAuditLogJson(): string {
  return JSON.stringify(inMemoryAuditLog, null, 2);
}

/**
 * Clears the in-memory audit log (primarily for isolated test environments).
 */
export function clearAuditLogForTesting(): void {
  inMemoryAuditLog.length = 0;
  eventCounter = 0;
}
