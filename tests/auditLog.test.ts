import { beforeEach, describe, expect, it } from 'vitest';
import {
  clearAuditLogForTesting,
  exportAuditLogJson,
  getAuditEventCount,
  getRecentAuditEvents,
  logAuditEvent,
} from '../electron/auditLog';
import { ApprovalQueue } from '../electron/approvals';

describe('Security Audit Logging Engine (electron/auditLog.ts)', () => {
  beforeEach(() => {
    clearAuditLogForTesting();
  });

  it('records an audit event with unique id, ISO timestamp, and actor', () => {
    const event = logAuditEvent({
      category: 'TOOL_INVOKED',
      actor: 'agent',
      sessionId: 'sess-001',
      details: { tool: 'search_game_tree', query: 'Baseplate' },
    });

    expect(event.id).toMatch(/^audit-\d+-\d+$/);
    expect(new Date(event.timestamp).getTime()).toBeGreaterThan(0);
    expect(event.category).toBe('TOOL_INVOKED');
    expect(event.actor).toBe('agent');
    expect(event.sessionId).toBe('sess-001');
    expect(event.details.tool).toBe('search_game_tree');
    expect(getAuditEventCount()).toBe(1);
  });

  it('automatically redacts sensitive details recorded in audit events', () => {
    const gKey = 'AIza' + 'SyA12345678901234567890123456789012';
    const event = logAuditEvent({
      category: 'TOOL_INVOKED',
      actor: 'agent',
      details: {
        command: 'rojo serve',
        token: gKey,
      },
    });

    expect(event.details.token).toBe('[REDACTED:GEMINI_KEY]');
  });

  it('filters recent events by category and respects limit bounds', () => {
    logAuditEvent({ category: 'APPROVAL_STAGED', actor: 'agent', details: { id: 'app-1' } });
    logAuditEvent({ category: 'TOOL_INVOKED', actor: 'agent', details: { tool: 'inspect_instance' } });
    logAuditEvent({ category: 'APPROVAL_GRANTED', actor: 'user', details: { id: 'app-1' } });
    logAuditEvent({ category: 'TOOL_INVOKED', actor: 'agent', details: { tool: 'multi_edit' } });

    expect(getRecentAuditEvents(10)).toHaveLength(4);
    const toolEvents = getRecentAuditEvents(10, 'TOOL_INVOKED');
    expect(toolEvents).toHaveLength(2);
    expect(toolEvents.every((e) => e.category === 'TOOL_INVOKED')).toBe(true);

    const limited = getRecentAuditEvents(2);
    expect(limited).toHaveLength(2);
    expect(limited[1].category).toBe('TOOL_INVOKED');
  });

  it('exports audit log to valid JSON formatted string', () => {
    logAuditEvent({ category: 'COMMAND_EXECUTION', actor: 'user', details: { command: 'npm test' } });
    const jsonStr = exportAuditLogJson();
    const parsed = JSON.parse(jsonStr) as Array<{ category: string }>;
    expect(Array.isArray(parsed)).toBe(true);
    expect(parsed[0].category).toBe('COMMAND_EXECUTION');
  });

  describe('ApprovalQueue Audit Trail Integration', () => {
    it('logs audit events across the complete approval lifecycle: stage, grant, reject, expire', () => {
      let currentTime = 10_000;
      const queue = new ApprovalQueue(() => currentTime, 5_000);

      // Stage write
      const write = queue.stageWrite({
        projectRoot: 'C:/proj',
        relPath: 'src/main.server.lua',
        content: 'print("hello")',
        previous: '',
      });
      expect(write.id).toBeDefined();

      let stagedEvents = getRecentAuditEvents(10, 'APPROVAL_STAGED');
      expect(stagedEvents).toHaveLength(1);
      expect(stagedEvents[0].details.id).toBe(write.id);
      expect(stagedEvents[0].details.kind).toBe('write');

      // Grant write
      const taken = queue.take(write.id);
      expect(taken.id).toBe(write.id);
      let grantedEvents = getRecentAuditEvents(10, 'APPROVAL_GRANTED');
      expect(grantedEvents).toHaveLength(1);
      expect(grantedEvents[0].details.id).toBe(write.id);

      // Stage command & reject
      const cmd = queue.stageCommand({
        projectRoot: 'C:/proj',
        command: 'rojo build',
        cwd: 'C:/proj',
      });
      const rejected = queue.reject(cmd.id);
      expect(rejected.id).toBe(cmd.id);
      let rejectedEvents = getRecentAuditEvents(10, 'APPROVAL_REJECTED');
      expect(rejectedEvents).toHaveLength(1);
      expect(rejectedEvents[0].details.id).toBe(cmd.id);

      // Stage MCP & expire
      const mcp = queue.stageMcp({
        projectRoot: 'C:/proj',
        tool: 'studio_read',
        args: { path: 'Workspace' },
      });
      expect(mcp.id).toBeDefined();

      // Fast forward past expiration
      currentTime += 6_000;
      expect(queue.list()).toEqual([]);
      let expiredEvents = getRecentAuditEvents(10, 'APPROVAL_EXPIRED');
      expect(expiredEvents).toHaveLength(1);
      expect(expiredEvents[0].details.id).toBe(mcp.id);
    });
  });
});
