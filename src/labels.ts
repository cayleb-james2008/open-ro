import type { PendingApprovalView } from '../electron/types';

/** Two-letter provider monogram. Parenthetical notes such as "(local)" are not part of the name. */
export function providerInitials(name: string): string {
  const words = name.replace(/\([^)]*\)/g, ' ').split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  if (words.length === 0) {
    return 'OP';
  }
  if (words.length === 1) {
    const word = words[0];
    return `${word[0].toUpperCase()}${word.slice(1, 2).toLowerCase()}`;
  }
  return `${words[0][0]}${words[1][0]}`.toUpperCase();
}

export interface ApprovalCopy {
  title: string;
  detail: string;
  preview: string;
  approveLabel: string;
}

/** Text for an approval card. Writes show a diff instead of `preview`. */
export function approvalCopy(item: PendingApprovalView): ApprovalCopy {
  switch (item.kind) {
    case 'write':
      return { title: `Review write: ${item.relPath ?? ''}`, detail: 'Nothing is written until you approve', preview: '', approveLabel: 'Approve write' };
    case 'mcp': {
      let preview = item.mcpArgs ?? '{}';
      try {
        preview = JSON.stringify(JSON.parse(preview), null, 2);
      } catch {
        // Show the stored text as is.
      }
      const detail = item.mcpServer
        ? `Runs on ${item.mcpServer} only if you approve these exact arguments. It will not run on any other server`
        : 'The tool runs on the connected MCP server only if you approve these exact arguments';
      return { title: `Review MCP tool: ${item.mcpTool ?? ''}`, detail, preview, approveLabel: 'Run tool' };
    }
    case 'mcp-server':
      return {
        title: 'Review MCP server start',
        detail: 'This program starts without a shell only if you approve',
        preview: [item.command ?? '', ...(item.serverArgs ?? []).map((arg) => (/[\s"']/.test(arg) ? JSON.stringify(arg) : arg))].join(' '),
        approveLabel: 'Start server',
      };
    case 'studio': {
      // Which Studio runs it: the studio_id when the server takes one, then the server command line.
      const where = `${item.studioId ? ` ${JSON.stringify(item.studioId)}` : ''}${item.mcpServer ? ` on ${item.mcpServer}` : ''}`;
      const target = `game.${item.studioPath ?? ''}`;
      if (item.studioOp === 'snapshot') {
        return { title: 'Review Studio read: place tree', detail: `Read-only Luau runs in Studio${where} only if you approve`, preview: `Services: ${(item.studioServices ?? []).join(', ')}`, approveLabel: 'Read place' };
      }
      if (item.studioOp === 'read') {
        return { title: `Review Studio read: ${target}`, detail: `Read-only Luau runs in Studio${where} only if you approve`, preview: target, approveLabel: 'Read script' };
      }
      const action = item.previous === undefined ? 'Creates this script' : 'Changes this script';
      const guard = item.previous === undefined ? 'It stops if a script appears there first' : 'It stops if the script changed since it was read';
      return {
        title: `Review Studio write: ${target} (${item.studioClass ?? ''})`,
        detail: `${action} in Studio${where} only if you approve. ${guard}. Ctrl+Z in Studio undoes it`,
        preview: '',
        approveLabel: 'Write to Studio',
      };
    }
    default:
      return { title: 'Review command', detail: item.cwd ?? '', preview: item.command ?? '', approveLabel: 'Run command' };
  }
}
