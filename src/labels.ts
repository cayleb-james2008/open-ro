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
    case 'delete':
      return { title: `Review delete: ${item.relPath ?? ''}`, detail: 'The file is removed only if you approve. Its current contents are checkpointed first', preview: '', approveLabel: 'Delete file' };
    case 'mcp': {
      if (item.mcpTool === 'media_generate' && item.mcpServer === 'openro-media') {
        let plan: { presetId?: unknown; prompt?: unknown; durationSeconds?: unknown; seed?: unknown } = {};
        try {
          const parsed: unknown = JSON.parse(item.mcpArgs ?? '{}');
          if (parsed && typeof parsed === 'object') {
            plan = parsed as typeof plan;
          }
        } catch {
          // A malformed plan still gets a safe card.
        }
        const preset = typeof plan.presetId === 'string' ? plan.presetId : 'ComfyUI audio';
        const prompt = typeof plan.prompt === 'string' ? plan.prompt.slice(0, 400) : '';
        const duration = typeof plan.durationSeconds === 'number' ? `${plan.durationSeconds}s` : 'audio';
        const seed = typeof plan.seed === 'number' ? `, seed ${plan.seed}` : '';
        return {
          title: `Generate local audio: ${preset}`,
          detail: `${duration}${seed} · runs on your loopback ComfyUI server only if you approve; nothing is downloaded`,
          preview: prompt,
          approveLabel: 'Generate audio',
        };
      }
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
      if (item.studioOp === 'play') {
        let starting = true;
        try {
          starting = (JSON.parse(item.mcpArgs ?? '{}') as { is_start?: unknown }).is_start === true;
        } catch {
          // Unreadable arguments: default to the start wording rather than throwing.
        }
        return {
          title: `Review Studio playtest: ${starting ? 'start' : 'stop'}`,
          detail: `${starting ? 'Starts' : 'Stops'} the running game in Studio${where} only if you approve`,
          preview: '',
          approveLabel: starting ? 'Start playtest' : 'Stop playtest',
        };
      }
      if (item.studioOp === 'console') {
        return { title: 'Review Studio console read', detail: `Reads the Studio output in Studio${where} only if you approve`, preview: '', approveLabel: 'Read output' };
      }
      if (item.studioOp === 'input') {
        const keyboard = item.mcpTool === 'user_keyboard_input';
        let total = 0;
        let steps: string[] = [];
        try {
          const parsed = JSON.parse(item.mcpArgs ?? '{}') as { actions?: Array<Record<string, unknown>> };
          const actions = Array.isArray(parsed.actions) ? parsed.actions : [];
          total = actions.length;
          steps = actions.map((step) => {
            const name = typeof step.action === 'string' ? step.action : 'action';
            const key = typeof step.key_code === 'string' ? ` ${step.key_code}` : '';
            let text = '';
            if (typeof step.text_inputs === 'string') {
              const raw = JSON.stringify(step.text_inputs);
              text = ` ${raw.length > 300 ? `${raw.slice(0, 300)}…` : raw}`;
            }
            const wait = typeof step.wait_time_ms === 'number' ? ` ${step.wait_time_ms}ms` : '';
            const button = typeof step.mouse_button === 'string' ? ` ${step.mouse_button}` : '';
            const coords = typeof step.x === 'number' && typeof step.y === 'number' ? ` (${step.x}, ${step.y})` : '';
            const target = typeof step.instance_path === 'string' ? ` @ ${step.instance_path}` : '';
            return `${name}${key}${text}${wait}${button}${coords}${target}`;
          });
        } catch {
          // Unreadable arguments: show the card without a step preview.
        }
        return {
          title: `Review Studio ${keyboard ? 'keyboard' : 'mouse'} input`,
          detail: `${total} step(s) go to the running playtest in Studio${where} only if you approve`,
          preview: steps.join('\n'),
          approveLabel: 'Send input',
        };
      }
      if (item.studioOp === 'capture') {
        return { title: 'Review Studio viewport capture', detail: `Captures the current viewport in Studio${where} only if you approve`, preview: '', approveLabel: 'Capture viewport' };
      }
      const action = item.previous === undefined ? 'Creates this script' : 'Changes this script';
      const guard = item.previous === undefined ? 'It stops if a script appears there first' : 'It stops if the script changed since it was read';
      return {
        title: `Review Studio write: ${target} (${item.studioClass ?? ''})`,
        detail: `${action} in Studio${where} only if you approve. ${guard}.`,
        preview: '',
        approveLabel: 'Write to Studio',
      };
    }
    default:
      return { title: 'Review command', detail: item.cwd ?? '', preview: item.command ?? '', approveLabel: 'Run command' };
  }
}
