export interface StoredTraceEntry {
  name: string;
  status: string;
  detail?: string;
}

export interface StoredChatMessage {
  role: 'user' | 'assistant';
  content: string;
  error?: string;
  trace?: StoredTraceEntry[];
}

export interface StoredChatSession {
  id: string;
  title: string;
  messages: StoredChatMessage[];
  updatedAt: string;
  /** Absolute project root this chat belongs to; absent for legacy or project-less chats. */
  projectRoot?: string;
}

export interface SessionSnapshot {
  activeId: string | null;
  sessions: StoredChatSession[];
}

const MAX_SESSIONS = 40;
const MAX_MESSAGES = 100;
const MAX_CONTENT = 20_000;
const MAX_PROJECT_ROOT = 1_000;

function asTrace(value: unknown): StoredTraceEntry[] | undefined {
  if (!Array.isArray(value)) {
    return undefined;
  }
  const trace: StoredTraceEntry[] = [];
  for (const entry of value.slice(0, 20)) {
    if (!entry || typeof entry !== 'object') {
      continue;
    }
    const item = entry as { name?: unknown; status?: unknown; detail?: unknown };
    if (typeof item.name !== 'string' || typeof item.status !== 'string') {
      continue;
    }
    trace.push({
      name: item.name.slice(0, 80),
      status: item.status.slice(0, 40),
      detail: typeof item.detail === 'string' ? item.detail.slice(0, 500) : undefined,
    });
  }
  return trace.length > 0 ? trace : undefined;
}

/** Drop malformed, oversized, or unexpected fields before chat history touches disk. */
export function sanitizeSessions(input: unknown): SessionSnapshot {
  const source = input && typeof input === 'object' && !Array.isArray(input) ? (input as { activeId?: unknown; sessions?: unknown }) : { sessions: input };
  const rawSessions = Array.isArray(source.sessions) ? source.sessions : Array.isArray(input) ? input : [];
  const sessions: StoredChatSession[] = [];
  for (const item of rawSessions.slice(0, MAX_SESSIONS)) {
    if (!item || typeof item !== 'object') {
      continue;
    }
    const record = item as { id?: unknown; title?: unknown; messages?: unknown; updatedAt?: unknown; projectRoot?: unknown };
    if (typeof record.id !== 'string' || !/^[\w.-]{1,80}$/.test(record.id)) {
      continue;
    }
    const title = typeof record.title === 'string' && record.title.trim() ? record.title.trim().slice(0, 80) : 'New chat';
    const messages: StoredChatMessage[] = [];
    if (Array.isArray(record.messages)) {
      for (const message of record.messages.slice(-MAX_MESSAGES)) {
        if (!message || typeof message !== 'object') {
          continue;
        }
        const row = message as { role?: unknown; content?: unknown; error?: unknown; trace?: unknown };
        if ((row.role !== 'user' && row.role !== 'assistant') || typeof row.content !== 'string') {
          continue;
        }
        messages.push({
          role: row.role,
          content: row.content.slice(0, MAX_CONTENT),
          error: typeof row.error === 'string' ? row.error.slice(0, 2_000) : undefined,
          trace: asTrace(row.trace),
        });
      }
    }
    sessions.push({
      id: record.id,
      title,
      messages,
      updatedAt: typeof record.updatedAt === 'string' ? record.updatedAt.slice(0, 40) : new Date().toISOString(),
      projectRoot: typeof record.projectRoot === 'string' && record.projectRoot.trim() ? record.projectRoot.trim().slice(0, MAX_PROJECT_ROOT) : undefined,
    });
  }
  const activeId = typeof source.activeId === 'string' && sessions.some((session) => session.id === source.activeId) ? source.activeId : (sessions[0]?.id ?? null);
  return { activeId, sessions };
}
