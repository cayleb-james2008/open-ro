import { spawn } from 'node:child_process';
import { scrubbedEnv } from './toolEnv';

export interface CommandResult {
  code: number | null;
  stdout: string;
  stderr: string;
  timedOut: boolean;
}

const MAX_OUTPUT = 20_000;

/**
 * Plan the shell that runs one approved command.
 *
 * On Windows the approved string is wrapped in one outer quote pair and cmd.exe
 * is started with /d /s /c: cmd strips only that outer pair, so interior quotes
 * survive and cmd's pipe/ampersand/caret parsing cannot split approved arguments
 * mid-string. The caller of spawn must pass windowsVerbatimArguments so Node
 * forwards the quote characters untouched (Node's default argument quoting
 * writes double quotes with backslash-escapes that cmd's parser misreads).
 */
export function shellInvocation(command: string, platform: NodeJS.Platform = process.platform): { file: string; args: string[] } {
  const trimmed = command.trim();
  if (!trimmed) {
    throw new Error('Command is empty.');
  }
  if (trimmed.length > 4_000) {
    throw new Error('Command is too long.');
  }
  if (platform === 'win32') {
    return { file: 'cmd.exe', args: ['/d', '/s', '/c', '"' + trimmed + '"'] };
  }
  return { file: '/bin/sh', args: ['-lc', trimmed] };
}

function cap(value: string): string {
  return value.length > MAX_OUTPUT ? value.slice(0, MAX_OUTPUT) : value;
}

/** Run one already-approved command. The caller must have checked the working directory. */
export function runShellCommand(options: { command: string; cwd: string; timeoutMs?: number }): Promise<CommandResult> {
  const requested = options.timeoutMs ?? 60_000;
  const timeoutMs = Math.min(Math.max(requested, 1_000), 5 * 60_000);
  const { file, args } = shellInvocation(options.command);
  return new Promise((resolve, reject) => {
    const child = spawn(file, args, {
      cwd: options.cwd,
      env: scrubbedEnv(),
      windowsHide: true,
      detached: process.platform !== 'win32',
      windowsVerbatimArguments: process.platform === 'win32',
    });
    let stdout = '';
    let stderr = '';
    let timedOut = false;
    let settled = false;
    const finish = (result: CommandResult) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      resolve(result);
    };
    const timer = setTimeout(() => {
      timedOut = true;
      if (child.pid && process.platform !== 'win32') {
        try {
          process.kill(-child.pid, 'SIGKILL');
        } catch {
          child.kill('SIGKILL');
        }
      } else if (child.pid) {
        // cmd.exe does not propagate kills to its descendants; ask taskkill
        // to terminate the whole process group, then force-stop cmd itself.
        // Whether taskkill exits (success or not) or fails to start, cmd is
        // always force-stopped so the promise still settles.
        const killer = spawn('taskkill.exe', ['/pid', String(child.pid), '/t', '/f'], {
          windowsHide: true,
          windowsVerbatimArguments: false,
        });
        const stopCmd = () => {
          child.kill('SIGKILL');
        };
        killer.on('exit', stopCmd);
        killer.on('error', stopCmd);
      } else {
        child.kill('SIGKILL');
      }
      // Never let a stalled taskkill keep this entry waiting forever; cmd
      // itself is always force-stopped two seconds after a timeout is observed.
      setTimeout(() => {
        if (!settled) {
          child.kill('SIGKILL');
        }
      }, 2_000);
    }, timeoutMs);
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk: string) => {
      stdout = cap(stdout + chunk);
    });
    child.stderr.on('data', (chunk: string) => {
      stderr = cap(stderr + chunk);
    });
    child.on('error', (error) => {
      if (settled) {
        return;
      }
      settled = true;
      clearTimeout(timer);
      reject(error);
    });
    child.on('exit', (code) => {
      finish({ code, stdout, stderr, timedOut });
    });
  });
}
