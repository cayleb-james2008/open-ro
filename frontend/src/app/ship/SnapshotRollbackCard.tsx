import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Check, FileDiff, History, Plus, RotateCcw, RotateCw, ShieldCheck } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  enc,
  projUrl,
  stamp,
  type ProjectSnapshotItem,
  type ProjectSnapshotRestoreResult,
  type ProjectSnapshotsResponse,
} from "./api";

type RawStudioDiffItem = { kind?: string; path?: string; detail?: string };
type RawStudioSnapshot = {
  id?: string;
  snapshot_id?: string;
  label?: string;
  created_at?: number;
  created_at_ms?: number;
  scripts_count?: number;
  verified?: boolean;
  assertion_summary?: ProjectSnapshotItem["assertion_summary"];
  changed_scripts?: ProjectSnapshotItem["changed_scripts"];
  assertion_delta?: ProjectSnapshotItem["assertion_delta"];
  diff?: { added?: RawStudioDiffItem[]; modified?: RawStudioDiffItem[]; removed?: RawStudioDiffItem[] };
};

function normalizeSnapshots(raw: Record<string, unknown>): {
  snapshots: ProjectSnapshotItem[];
  latestVerifiedId: string | null;
} {
  const arr = Array.isArray(raw.snapshots) ? (raw.snapshots as RawStudioSnapshot[]) : [];
  const snapshots: ProjectSnapshotItem[] = arr
    .map((s) => {
      const sid = s.snapshot_id || s.id || "";
      const ms =
        typeof s.created_at_ms === "number"
          ? s.created_at_ms
          : typeof s.created_at === "number"
            ? (s.created_at < 1e12 ? s.created_at * 1000 : s.created_at)
            : Date.now();
      let changed = Array.isArray(s.changed_scripts) ? s.changed_scripts : [];
      if (changed.length === 0 && s.diff) {
        const mapKind = (items: RawStudioDiffItem[] | undefined, op: string) =>
          (items || [])
            .filter((d) => d.kind === "script" || !d.kind)
            .map((d) => ({ path: d.path || "Script", op, before_lines: 0, after_lines: 0 }));
        changed = [
          ...mapKind(s.diff.added, "added"),
          ...mapKind(s.diff.modified, "modified"),
          ...mapKind(s.diff.removed, "removed"),
        ];
      }
      return {
        snapshot_id: sid,
        label: s.label || "Snapshot",
        created_at_ms: ms,
        scripts_count: s.scripts_count ?? 0,
        verified: typeof s.verified === "boolean" ? s.verified : true,
        assertion_summary: s.assertion_summary,
        changed_scripts: changed,
        assertion_delta: s.assertion_delta ?? {
          passed_delta: 0,
          prev_passed_count: 0,
          curr_passed_count: 0,
          newly_passed: [],
          newly_failed: [],
        },
      };
    })
    .filter((s) => Boolean(s.snapshot_id));
  const explicitLatest = typeof raw.latest_verified_id === "string" ? raw.latest_verified_id : null;
  const fallbackLatest = snapshots.find((s) => s.verified)?.snapshot_id ?? null;
  return { snapshots, latestVerifiedId: explicitLatest || fallbackLatest };
}

export function SnapshotRollbackCard({
  projectId,
  studioOk = true,
  onRestored,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  onRestored?: (runId?: string) => Promise<void> | void;
}) {
  const [snapshots, setSnapshots] = useState<ProjectSnapshotItem[]>([]);
  const [latestVerifiedId, setLatestVerifiedId] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saving, setSaving] = useState(false);
  const [restoringId, setRestoringId] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const endpoint = projectId ? projUrl(projectId, "/snapshots") : "/api/snapshots";
    const res = await call<ProjectSnapshotsResponse>(endpoint);
    setBusy(false);
    if (res.ok) {
      const parsed = normalizeSnapshots(res as unknown as Record<string, unknown>);
      setSnapshots(parsed.snapshots);
      setLatestVerifiedId(parsed.latestVerifiedId);
    }
  }, [projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const createCheckpoint = async () => {
    if (!projectId && !studioOk) return;
    setSaving(true);
    const endpoint = projectId ? projUrl(projectId, "/snapshots") : "/api/snapshots";
    const res = await call<ProjectSnapshotsResponse>(endpoint, {
      label: `Checkpoint ${snapshots.length + 1}`,
    });
    setSaving(false);
    if (!res.ok) {
      toast.error(res.error || "Could not save playable snapshot.");
      return;
    }
    const parsed = normalizeSnapshots(res as unknown as Record<string, unknown>);
    if (parsed.snapshots.length > 0) {
      setSnapshots(parsed.snapshots);
      setLatestVerifiedId(parsed.latestVerifiedId);
    } else {
      await load();
    }
    toast.success("Saved playable checkpoint.");
  };

  const restoreSnapshot = async (sid: string, label?: string) => {
    if ((!projectId && !studioOk) || !sid) return;
    setRestoringId(sid);
    const endpoint = projectId
      ? projUrl(projectId, `/snapshots/${enc(sid)}/restore`)
      : `/api/snapshots/${enc(sid)}/restore`;
    const res = await call<ProjectSnapshotRestoreResult>(endpoint, { auto_checkpoint: true });
    setRestoringId(null);
    if (!res.ok) {
      toast.error(res.error || "Could not restore snapshot.");
      return;
    }
    const parsed = normalizeSnapshots(res as unknown as Record<string, unknown>);
    if (parsed.snapshots.length > 0) {
      setSnapshots(parsed.snapshots);
      setLatestVerifiedId(parsed.latestVerifiedId);
    } else {
      await load();
    }
    await onRestored?.(res.rerun?.run_id);
    toast.success(
      `Restored "${res.label || label || sid}"` +
        (res.restored_files ? ` (${res.restored_files.length} scripts)` : ""),
    );
  };

  const latestVerified =
    snapshots.find((s) => s.snapshot_id === latestVerifiedId) ??
    snapshots.find((s) => s.verified) ??
    null;

  return (
    <section
      data-snapshot-rollback
      aria-label="Playable Snapshot Diff and Safe Rollback"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <History className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">
              Playable Snapshot Diff &amp; Safe Rollback
            </h3>
            <Pill
              className={
                latestVerified
                  ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                  : "bg-board px-2 py-0.5 text-xs text-ink-2"
              }
            >
              {latestVerified ? (
                <>
                  <ShieldCheck className="size-3" strokeWidth={2} aria-hidden />
                  <span>{snapshots.length} saved · verified ready</span>
                </>
              ) : (
                <span>{snapshots.length} saved</span>
              )}
            </Pill>
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Compare Luau script diffs and assertion deltas across checkpoints, or restore your last verified playable snapshot in one click.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            loading={busy}
            onClick={() => void load()}
            data-refresh-snapshots
            className="max-md:min-h-11"
          >
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Refresh
          </Button>
          <Button
            variant="outline"
            size="sm"
            loading={saving}
            disabled={saving || Boolean(restoringId)}
            onClick={() => void createCheckpoint()}
            data-save-project-snapshot
            className="max-md:min-h-11"
          >
            <Plus className="size-3.5" strokeWidth={1.5} aria-hidden />
            {saving ? "Saving…" : "Save Snapshot"}
          </Button>
          <Button
            variant={latestVerified ? "default" : "outline"}
            size="sm"
            loading={Boolean(latestVerified && restoringId === latestVerified.snapshot_id)}
            disabled={!latestVerified || Boolean(restoringId)}
            onClick={() => latestVerified && void restoreSnapshot(latestVerified.snapshot_id, latestVerified.label)}
            data-restore-verified-snapshot
            className="max-md:min-h-11"
          >
            <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Restore Verified Snapshot
          </Button>
        </div>
      </div>

      {snapshots.length === 0 ? (
        <div className="rounded-sm border border-hairline bg-board/40 px-3 py-2.5 text-xs text-ink-2">
          No checkpoints saved yet. Run a playtest or save a snapshot to track script diffs and enable 1-click rollback.
        </div>
      ) : (
        <ul className="flex flex-col gap-2" aria-label="Playable snapshot timeline">
          {snapshots.slice(0, 6).map((snap) => {
            const passed = snap.assertion_summary?.passed_count;
            const total = snap.assertion_summary?.total;
            const delta = snap.assertion_delta?.passed_delta ?? 0;
            const newlyPassed = snap.assertion_delta?.newly_passed ?? [];
            const newlyFailed = snap.assertion_delta?.newly_failed ?? [];
            return (
              <li
                key={snap.snapshot_id}
                data-snapshot-row={snap.snapshot_id}
                className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/40 px-3 py-2.5"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="truncate font-heading text-xs font-medium text-ink">
                        {snap.label}
                      </span>
                      <Pill
                        className={
                          snap.verified
                            ? "bg-pill-active-bg px-1.5 py-0.5 text-[11px] text-pill-active-fg"
                            : "bg-pill-warn-bg px-1.5 py-0.5 text-[11px] text-pill-warn-fg"
                        }
                      >
                        {snap.verified ? (
                          <>
                            <Check className="size-3" strokeWidth={2} aria-hidden />
                            <span>Verified</span>
                          </>
                        ) : typeof passed === "number" && typeof total === "number" ? (
                          <span>
                            {passed}/{total} checks
                          </span>
                        ) : (
                          <span>Unverified</span>
                        )}
                      </Pill>
                      {delta !== 0 && (
                        <Pill
                          className={
                            delta > 0
                              ? "bg-pill-active-bg px-1.5 py-0.5 font-mono text-[11px] text-pill-active-fg tabular-nums"
                              : "bg-pill-warn-bg px-1.5 py-0.5 font-mono text-[11px] text-pill-warn-fg tabular-nums"
                          }
                        >
                          {delta > 0 ? `+${delta} passed` : `${delta} passed`}
                        </Pill>
                      )}
                    </div>
                    <div className="mt-0.5 flex flex-wrap items-center gap-2 text-[11px] text-ink-2 tabular-nums">
                      <span>{stamp(snap.created_at_ms)}</span>
                      <span>·</span>
                      <span>{snap.scripts_count} scripts</span>
                      {newlyPassed.length > 0 && (
                        <span className="text-ink">+{newlyPassed.length} fixed</span>
                      )}
                      {newlyFailed.length > 0 && (
                        <span className="text-pill-pro-fg">-{newlyFailed.length} regressed</span>
                      )}
                    </div>
                  </div>
                  <Button
                    variant="outline"
                    size="sm"
                    loading={restoringId === snap.snapshot_id}
                    disabled={Boolean(restoringId)}
                    onClick={() => void restoreSnapshot(snap.snapshot_id, snap.label)}
                    data-restore-snapshot={snap.snapshot_id}
                    className="max-md:min-h-11"
                  >
                    <RotateCcw className="size-3.5" strokeWidth={1.5} aria-hidden />
                    Restore
                  </Button>
                </div>

                {snap.changed_scripts.length > 0 ? (
                  <div className="flex flex-wrap items-center gap-1.5" data-snapshot-diff={snap.snapshot_id}>
                    <FileDiff className="size-3.5 shrink-0 text-ink-2" strokeWidth={1.5} aria-hidden />
                    {snap.changed_scripts.slice(0, 4).map((ch, idx) => (
                      <span
                        key={`${ch.path}:${idx}`}
                        className="rounded-sm border border-hairline bg-surface px-2 py-0.5 font-mono text-[11px] text-ink tabular-nums"
                      >
                        {ch.op === "added" ? "+" : ch.op === "removed" ? "-" : "~"}
                        {ch.path}
                        {ch.before_lines || ch.after_lines
                          ? ` (${ch.before_lines}→${ch.after_lines}L)`
                          : ""}
                      </span>
                    ))}
                    {snap.changed_scripts.length > 4 && (
                      <span className="text-[11px] text-ink-2 tabular-nums">
                        +{snap.changed_scripts.length - 4} more
                      </span>
                    )}
                  </div>
                ) : (
                  <div className="text-[11px] text-ink-2" data-snapshot-diff={snap.snapshot_id}>
                    No Luau script changes relative to previous checkpoint.
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
