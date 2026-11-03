import type { ApprovalDecision, ApprovalKind } from './approvals';

type Resolve<T> = (id: string, decision: ApprovalDecision, kind?: ApprovalKind) => T;
type Handle = (channel: string, listener: (event: unknown, args: unknown) => unknown) => void;

/** Run channels approve only their own kind. Any other kind is refused and stays pending. */
export const RUN_CHANNEL_KINDS = { 'project:write': 'write', 'command:run': 'command' } as const satisfies Record<string, ApprovalKind>;

/** Register the approval IPC channels. `approvals:resolve` is the one path that takes any kind. */
export function registerApprovalChannels<T>(handle: Handle, resolve: Resolve<T>): void {
  for (const [channel, kind] of Object.entries(RUN_CHANNEL_KINDS)) {
    handle(channel, (_event, args) => resolve((args as { id: string }).id, 'approve', kind));
  }
  handle('approvals:resolve', (_event, args) => {
    const input = args as { id?: unknown; decision?: unknown } | undefined;
    if (!input || (input.decision !== 'approve' && input.decision !== 'reject') || typeof input.id !== 'string') {
      throw new Error('Approval decision is invalid.');
    }
    return resolve(input.id, input.decision);
  });
}
