import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Archive, ArrowLeftRight, Check, FileDiff, GitMerge, RotateCcw, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  projUrl,
  type SyncConflictItem,
  type SyncConflictResolveResult,
  type SyncConflictsReport,
} from "./api";

export function SyncConflictsCard({
  projectId,
  studioOk = true,
  hideWhenInSync = false,
  onResolved,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenInSync?: boolean;
  onResolved?: () => Promise<void> | void;
}) {
  const [report, setReport] = useState<SyncConflictsReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [resolvingKey, setResolvingKey] = useState<string | null>(null);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const endpoint = projectId ? projUrl(projectId, "/sync-conflicts") : "/api/studio/sync-conflicts";
    const res = await call<SyncConflictsReport>(endpoint);
    setBusy(false);
    if (res.ok && typeof res.in_sync === "boolean") {
      setReport(res as SyncConflictsReport);
      if (Array.isArray(res.conflicts) && res.conflicts.length > 0 && !expandedPath) {
        setExpandedPath(res.conflicts[0].path);
      }
    }
  }, [projectId, studioOk, expandedPath]);

  useEffect(() => {
    void load();
  }, [load]);

  const resolve = async (
    item: SyncConflictItem,
    action: "accept_local" | "keep_place" | "backup_snapshot",
  ) => {
    if (!projectId && !studioOk) return;
    setResolvingKey(`${item.path}:${action}`);
    const endpoint = projectId
      ? projUrl(projectId, "/sync-conflicts/resolve")
      : "/api/studio/sync-conflicts/resolve";
    const res = await call<SyncConflictResolveResult>(endpoint, {
      path: item.path,
      action,
    });
    setResolvingKey(null);
    if (!res.ok) {
      toast.error(res.error || "Could not resolve sync conflict.");
      return;
    }
    if (res.report) {
      setReport(res.report);
    } else {
      await load();
    }
    await onResolved?.();
    const label =
      action === "accept_local"
        ? "Accepted Local edit"
        : action === "keep_place"
          ? "Kept Place baseline (with backup)"
          : "Saved conflict backup snapshot";
    toast.success(`${label}: ${item.path}`);
  };

  if (hideWhenInSync && (!report || !report.has_baseline || report.in_sync)) {
    return null;
  }

  const conflicts = report?.conflicts ?? [];

  return (
    <section
      data-sync-conflicts
      aria-label="Rojo Live-Sync Conflict Resolver and Script Diff Preview"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <ArrowLeftRight className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">
              Rojo Live-Sync Conflict Resolver
            </h3>
            {report && (
              <Pill
                className={
                  !report.has_baseline
                    ? "bg-board px-2 py-0.5 text-xs text-ink-2"
                    : report.in_sync
                      ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                      : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
              >
                {!report.has_baseline ? (
                  <span>Awaiting sync baseline</span>
                ) : report.in_sync ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>0 sync conflicts</span>
                  </>
                ) : (
                  <span>{report.conflict_count} script conflict(s)</span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Detects Luau drift between local Rojo files and your synced Place baseline. Preview line diffs, then accept local edits, keep the Place baseline, or save a backup snapshot.
          </p>
        </div>
        <Button
          variant="outline"
          size="sm"
          loading={busy}
          onClick={() => void load()}
          data-refresh-sync-conflicts
          className="max-md:min-h-11"
        >
          <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
          Refresh
        </Button>
      </div>

      {conflicts.length === 0 ? (
        <div className="rounded-sm border border-hairline bg-board/40 px-3 py-2 text-xs text-ink-2">
          {report?.has_baseline
            ? "Local Rojo workspace scripts match the active synced Place baseline cleanly."
            : "Save a snapshot or sync once to establish a baseline for drift detection."}
        </div>
      ) : (
        <ul className="flex flex-col gap-2.5" aria-label="Detected Rojo script sync conflicts">
          {conflicts.map((c) => {
            const open = expandedPath === c.path || conflicts.length === 1;
            const localLines = c.local_source ? c.local_source.split("\n").slice(0, 12) : [];
            const placeLines = c.place_source ? c.place_source.split("\n").slice(0, 12) : [];
            return (
              <li
                key={c.path}
                data-sync-conflict-row={c.path}
                className="flex flex-col gap-2 rounded-sm border border-hairline bg-board/40 p-3"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <FileDiff className="size-3.5 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
                      <span className="truncate font-mono text-xs font-medium text-ink">{c.path}</span>
                      <Pill className="bg-pill-warn-bg px-1.5 py-0.5 text-[11px] text-pill-warn-fg">
                        {c.status} · +{c.added_lines}/-{c.removed_lines}L
                      </Pill>
                    </div>
                    <div className="mt-0.5 font-mono text-[11px] text-ink-2">
                      Place target: {c.place_path}
                    </div>
                  </div>
                  <div className="flex flex-wrap items-center gap-1.5">
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => setExpandedPath(open ? null : c.path)}
                      className="max-md:min-h-11"
                    >
                      {open ? "Hide Diff" : "Preview Diff"}
                    </Button>
                    <Button
                      variant="default"
                      size="xs"
                      loading={resolvingKey === `${c.path}:accept_local`}
                      disabled={Boolean(resolvingKey)}
                      onClick={() => void resolve(c, "accept_local")}
                      data-resolve-accept-local={c.path}
                      className="max-md:min-h-11"
                    >
                      <GitMerge className="size-3" strokeWidth={1.5} aria-hidden />
                      Accept Local
                    </Button>
                    <Button
                      variant="outline"
                      size="xs"
                      loading={resolvingKey === `${c.path}:keep_place`}
                      disabled={Boolean(resolvingKey)}
                      onClick={() => void resolve(c, "keep_place")}
                      data-resolve-keep-place={c.path}
                      className="max-md:min-h-11"
                    >
                      <RotateCcw className="size-3" strokeWidth={1.5} aria-hidden />
                      Keep Place
                    </Button>
                    <Button
                      variant="outline"
                      size="xs"
                      loading={resolvingKey === `${c.path}:backup_snapshot`}
                      disabled={Boolean(resolvingKey)}
                      onClick={() => void resolve(c, "backup_snapshot")}
                      data-resolve-backup-snapshot={c.path}
                      className="max-md:min-h-11"
                    >
                      <Archive className="size-3" strokeWidth={1.5} aria-hidden />
                      Backup Snapshot
                    </Button>
                  </div>
                </div>

                {open && (
                  <div className="flex flex-col gap-2" data-sync-hunks={c.path}>
                    <div className="grid gap-2 md:grid-cols-2">
                      <div className="rounded-sm border border-hairline bg-surface p-2">
                        <div className="mb-1 font-mono text-[11px] font-medium text-ink-2">
                          Place Baseline ({c.place_path})
                        </div>
                        <pre className="max-h-36 overflow-auto font-mono text-[11px] leading-relaxed text-ink">
                          {placeLines.length ? placeLines.join("\n") : "-- (empty / not in baseline)"}
                        </pre>
                      </div>
                      <div className="rounded-sm border border-hairline bg-surface p-2">
                        <div className="mb-1 font-mono text-[11px] font-medium text-ink-2">
                          Local Workspace ({c.path})
                        </div>
                        <pre className="max-h-36 overflow-auto font-mono text-[11px] leading-relaxed text-ink">
                          {localLines.length ? localLines.join("\n") : "-- (deleted on disk)"}
                        </pre>
                      </div>
                    </div>
                    {c.hunks.length > 0 && (
                      <div className="rounded-sm border border-hairline bg-surface p-2 font-mono text-[11px]">
                        {c.hunks.slice(0, 2).map((h, hi) => (
                          <div key={hi} className="mb-1 last:mb-0">
                            <div className="text-ink-3">{h.header}</div>
                            {h.lines.slice(0, 12).map((l, li) => (
                              <div
                                key={li}
                                className={
                                  l.op === "add"
                                    ? "bg-pill-active-bg/30 text-pill-active-fg"
                                    : l.op === "del"
                                      ? "bg-pill-warn-bg/30 text-pill-pro-fg"
                                      : "text-ink-2"
                                }
                              >
                                {l.op === "add" ? "+" : l.op === "del" ? "-" : " "}
                                {l.text}
                              </div>
                            ))}
                          </div>
                        ))}
                      </div>
                    )}
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
