import * as crypto from 'node:crypto';
import * as fs from 'node:fs/promises';
import * as path from 'node:path';

const TRANSIENT_RENAME_ERRORS = new Set(['EPERM', 'EBUSY', 'EACCES']);
const RENAME_ATTEMPTS = 5;

/** In-process write queue per target: concurrent saves to one file run in order. */
const writeChains = new Map<string, Promise<void>>();

async function renameWithRetry(from: string, to: string): Promise<void> {
  for (let attempt = 0; ; attempt += 1) {
    try {
      await fs.rename(from, to);
      return;
    } catch (error) {
      const code = (error as NodeJS.ErrnoException).code ?? '';
      if (!TRANSIENT_RENAME_ERRORS.has(code) || attempt >= RENAME_ATTEMPTS - 1) {
        throw error;
      }
      // Windows can briefly hold the target (indexers, antivirus); back off and retry.
      await new Promise((resolve) => setTimeout(resolve, 10 * 2 ** attempt));
    }
  }
}

async function writeNow(filePath: string, text: string, mode: number): Promise<void> {
  await fs.mkdir(path.dirname(filePath), { recursive: true });
  const tempPath = `${filePath}.${process.pid}.${crypto.randomBytes(6).toString('hex')}.tmp`;
  try {
    const handle = await fs.open(tempPath, 'w', mode);
    try {
      await handle.writeFile(text, 'utf8');
      await handle.sync();
    } finally {
      await handle.close();
    }
    await renameWithRetry(tempPath, filePath);
  } catch (error) {
    await fs.rm(tempPath, { force: true }).catch(() => undefined);
    throw error;
  }
}

/**
 * Write text so a crash cannot leave a truncated target file: the full content
 * is written and flushed to a uniquely named sibling, then renamed over the
 * target. Readers see either the previous complete file or the new one; a
 * failed write cleans up its temp file and leaves the target untouched.
 *
 * Writes to the same path are serialized so rapid autosaves cannot race each
 * other into a Windows EPERM on rename.
 */
export async function writeTextAtomic(filePath: string, text: string, mode = 0o600): Promise<void> {
  const previous = writeChains.get(filePath) ?? Promise.resolve();
  let release!: () => void;
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  const tail = previous.then(() => gate, () => gate);
  writeChains.set(filePath, tail);
  await previous.catch(() => undefined);
  try {
    await writeNow(filePath, text, mode);
  } finally {
    release();
    if (writeChains.get(filePath) === tail) {
      writeChains.delete(filePath);
    }
  }
}
