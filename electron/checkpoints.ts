import * as path from 'node:path';
import type { ApprovalQueue, PendingApproval } from './approvals';

/** One checkpoint record: the text a file had before an approved write. */
export interface CheckpointRecord {
  projectRoot: string;
  relPath: string;
  previous: string | null;
}

/** The write needed to put the checkpointed text back. */
export interface FileRestorePlan {
  relPath: string;
  content: string;
  previous: string;
}

/**
 * Parse and validate one checkpoint JSON record. The record must belong to the
 * project the user has open; otherwise a stale checkpoint could rewrite a file
 * in a different project.
 */
export function parseCheckpointRecord(raw: string, expectedRoot: string): CheckpointRecord {
  const data = JSON.parse(raw) as Partial<CheckpointRecord> | null;
  if (
    !data ||
    typeof data.projectRoot !== 'string' ||
    typeof data.relPath !== 'string' ||
    !(data.previous === null || typeof data.previous === 'string')
  ) {
    throw new Error('Checkpoint record is invalid.');
  }
  if (path.resolve(data.projectRoot) !== path.resolve(expectedRoot)) {
    throw new Error('Checkpoint belongs to a different project.');
  }
  return { projectRoot: data.projectRoot, relPath: data.relPath, previous: data.previous };
}

/**
 * Plan the write that restores a checkpoint. The plan is staged as an ordinary
 * write approval, so the user reviews the diff first and the state being
 * overwritten is itself checkpointed — a restore can be restored.
 */
export function planFileRestore(record: CheckpointRecord, current: string): FileRestorePlan {
  if (record.previous === null) {
    throw new Error(
      'This checkpoint records a file that Open-Ro created. Restoring it would delete the file; delete the file from your file manager instead. Nothing was changed.',
    );
  }
  return { relPath: record.relPath, content: record.previous, previous: current };
}

/**
 * Stage a checkpoint restore as an ordinary write approval. The IPC handler
 * calls this instead of writing the target itself, so every restore passes
 * through the same review gate, diff preview, and fresh checkpoint as a write.
 */
export function stageCheckpointRestore(
  approvals: ApprovalQueue,
  input: { projectRoot: string; record: CheckpointRecord; current: string },
): PendingApproval {
  return approvals.stageWrite({ projectRoot: input.projectRoot, ...planFileRestore(input.record, input.current) });
}
