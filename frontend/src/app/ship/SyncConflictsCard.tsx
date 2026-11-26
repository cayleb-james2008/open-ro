import { useCallback, useEffect, useMemo, useState } from "react";
import { toast } from "sonner";
import { Archive, ArrowDownToLine, ArrowLeftRight, Check, FileDiff, GitMerge, RotateCcw, RotateCw } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import {
  call,
  projUrl,
  pullSyncApply,
  pullSyncDiff,
  type StudioPullChange,
  type StudioPullHunk,
  type StudioPullResponse,
  type StudioPullStrategy,
  type SyncConflictItem,
  type SyncConflictResolveResult,
  type SyncConflictsReport,
} from "./api";

const PULL_STATUS_META: Record<string, { label: string; cls: string }> = {
  studio_only: { label: "Studio Only", cls: "bg-pill-active-bg text-pill-active-fg" },
  studio_modified: { label: "Studio Modified", cls: "bg-tag-sky-bg text-tag-sky-fg" },
  local_only: { label: "Local Only", cls: "bg-board text-ink-2" },
  conflict: { label: "Conflict", cls: "bg-pill-warn-bg text-pill-warn-fg" },
};

const PULL_STRATEGIES: Array<{ id: StudioPullStrategy; label: string }> = [
  { id: "merge_non_conflicting", label: "Merge Non-Conflicting" },
  { id: "take_studio", label: "Take Studio" },
  { id: "keep_local", label: "Keep Local" },
];

function formatShortHash(val: number | null | undefined): string | null {
  if (typeof val !== "number" || !Number.isFinite(val)) return null;
  return (val >>> 0).toString(16).padStart(8, "0").slice(0, 8);
}

function renderPullHunk(hunk: StudioPullHunk, idx: number) {
  if (typeof hunk === "string") {
    return (
      <div key={idx} className="whitespace-pre-wrap text-ink-2">
        {hunk}
      </div>
    );
  }
  if (hunk && typeof hunk === "object" && Array.isArray(hunk.lines)) {
    return (
      <div key={idx} className="mb-1 last:mb-0">
        {hunk.header && <div className="text-ink-3">{hunk.header}</div>}
        {hunk.lines.slice(0, 16).map((l, li) => (
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
    );
  }
  if (hunk && typeof hunk === "object" && "text" in hunk && typeof hunk.text === "string") {
    const op = "op" in hunk ? String(hunk.op || "") : "";
    return (
      <div
        key={idx}
        className={
          op === "add"
            ? "bg-pill-active-bg/30 text-pill-active-fg"
            : op === "del"
              ? "bg-pill-warn-bg/30 text-pill-pro-fg"
              : "text-ink-2"
        }
      >
        {op === "add" ? "+" : op === "del" ? "-" : " "}
        {hunk.text}
      </div>
    );
  }
  return (
    <pre key={idx} className="overflow-x-auto whitespace-pre-wrap text-ink-2">
      {JSON.stringify(hunk, null, 2)}
    </pre>
  );
}

export function SyncConflictsCard({
  projectId,
  studioOk = true,
  hideWhenInSync = false,
  autoPullOnMount = false,
  onResolved,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenInSync?: boolean;
  autoPullOnMount?: boolean;
  onResolved?: () => Promise<void> | void;
}) {
  const [report, setReport] = useState<SyncConflictsReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [resolvingKey, setResolvingKey] = useState<string | null>(null);
  const [expandedPath, setExpandedPath] = useState<string | null>(null);

  const [pullBusy, setPullBusy] = useState(false);
  const [applyBusy, setApplyBusy] = useState(false);
  const [pullError, setPullError] = useState("");
  const [pullReport, setPullReport] = useState<StudioPullResponse | null>(null);
  const [strategy, setStrategy] = useState<StudioPullStrategy>("merge_non_conflicting");
  const [selectedPaths, setSelectedPaths] = useState<Record<string, boolean>>({});
  const [confirmOverwrite, setConfirmOverwrite] = useState(false);
  const [expandedPullPath, setExpandedPullPath] = useState<string | null>(null);

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

  const applySelectionFromChanges = useCallback((changes: StudioPullChange[]) => {
    const nextSel: Record<string, boolean> = {};
    for (const c of changes) {
      if (c.status === "studio_only" || c.status === "studio_modified") {
        nextSel[c.path] = true;
      }
    }
    setSelectedPaths(nextSel);
    setConfirmOverwrite(false);
    if (changes.length > 0) {
      setExpandedPullPath(changes[0].path);
    }
  }, []);

  const runPullDiff = useCallback(async () => {
    if (!projectId) {
      toast.error("Open a project to pull changes from Studio.");
      return;
    }
    setPullBusy(true);
    setPullError("");
    const res = await pullSyncDiff(projectId);
    setPullBusy(false);
    if (!res.ok) {
      const msg = res.error || "Could not inspect Studio changes.";
      setPullError(msg);
      toast.error(msg);
      return;
    }
    const nextReport: StudioPullResponse = {
      ok: true,
      snapshot_id: res.snapshot_id || "studio-snap",
      changes: Array.isArray(res.changes) ? res.changes : [],
      applied_count: res.applied_count ?? 0,
      conflict_count: res.conflict_count ?? 0,
    };
    setPullReport(nextReport);
    applySelectionFromChanges(nextReport.changes);
  }, [applySelectionFromChanges, projectId]);

  useEffect(() => {
    void load();
  }, [load]);

  useEffect(() => {
    if (autoPullOnMount && projectId) {
      void runPullDiff();
    }
  }, [autoPullOnMount, projectId, runPullDiff]);

  const pullChanges = pullReport?.changes ?? [];
  const chosenPaths = useMemo(
    () => pullChanges.filter((c) => selectedPaths[c.path]).map((c) => c.path),
    [pullChanges, selectedPaths],
  );
  const needsOverwriteConfirm = useMemo(() => {
    if (strategy !== "take_studio") return false;
    return pullChanges.some((c) => selectedPaths[c.path] && (c.status === "conflict" || c.status === "local_only"));
  }, [pullChanges, selectedPaths, strategy]);

  const handlePullApply = async () => {
    if (!projectId) return;
    if (chosenPaths.length === 0) {
      toast.error("Select at least one change to apply.");
      return;
    }
    if (needsOverwriteConfirm && !confirmOverwrite) {
      toast.error("Check confirm overwrite before replacing conflicting local files.");
      return;
    }
    setApplyBusy(true);
    setPullError("");
    const res = await pullSyncApply(projectId, {
      paths: chosenPaths,
      strategy,
      confirm_overwrite: needsOverwriteConfirm ? confirmOverwrite : undefined,
    });
    setApplyBusy(false);
    if (!res.ok) {
      const msg = res.error || "Could not apply Studio pull changes.";
      setPullError(msg);
      toast.error(msg);
      return;
    }
    const nextReport: StudioPullResponse = {
      ok: true,
      snapshot_id: res.snapshot_id || pullReport?.snapshot_id || "studio-snap",
      changes: Array.isArray(res.changes) ? res.changes : [],
      applied_count: res.applied_count ?? 0,
      conflict_count: res.conflict_count ?? 0,
    };
    setPullReport(nextReport);
    applySelectionFromChanges(nextReport.changes);
    await load();
    await onResolved?.();
    toast.success(`Two-way sync applied ${nextReport.applied_count} change(s).`);
  };

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

  if (hideWhenInSync && !pullReport && (!report || !report.has_baseline || report.in_sync)) {
    return null;
  }

  const conflicts = report?.conflicts ?? [];

  return (
    <section
      data-sync-conflicts
      aria-label="Two-Way Studio Sync and Conflict Resolver"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <ArrowLeftRight className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">
              Two-Way Studio Sync &amp; Conflict Resolver
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
                    <span>0 script conflicts</span>
                  </>
                ) : (
                  <span>{report.conflict_count} script conflict(s)</span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Pull Studio edits back into your Rojo project with 3-way conflict detection, diff previews, and selective apply, or resolve local script drift against the synced baseline.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          {projectId && (
            <Button
              variant="default"
              size="sm"
              loading={pullBusy}
              disabled={applyBusy}
              onClick={() => void runPullDiff()}
              data-pull-sync-diff
              className="max-md:min-h-11"
            >
              <ArrowDownToLine className="size-3.5" strokeWidth={1.5} aria-hidden />
              Pull from Studio
            </Button>
          )}
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
      </div>

      {pullError && (
        <p role="alert" data-pull-error className="rounded-sm bg-pill-pro-bg px-3 py-2 text-xs text-pill-pro-fg">
          {pullError}
        </p>
      )}

      {pullReport && (
        <div
          data-pull-results
          className="flex flex-col gap-2.5 rounded-sm border border-hairline bg-board/50 p-3"
        >
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex flex-wrap items-center gap-2">
              <span className="text-xs font-medium text-ink">Studio Pull Preview</span>
              <Pill className="bg-surface px-2 py-0.5 font-mono text-xs text-ink-2">
                {pullReport.snapshot_id}
              </Pill>
              <Pill className="bg-tag-sky-bg px-2 py-0.5 text-xs text-tag-sky-fg">
                {pullChanges.length} change(s)
              </Pill>
              {pullReport.conflict_count > 0 && (
                <Pill className="bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg">
                  {pullReport.conflict_count} conflict(s)
                </Pill>
              )}
            </div>

            <SegmentedControl aria-label="Two-way pull sync strategy" size="sm" data-pull-strategy className="max-w-full flex-wrap">
              {PULL_STRATEGIES.map((opt) => (
                <SegmentedControlItem
                  key={opt.id}
                  data-checked={strategy === opt.id ? "" : undefined}
                  aria-pressed={strategy === opt.id}
                  data-pull-strategy-opt={opt.id}
                  onClick={() => setStrategy(opt.id)}
                  className="max-md:!h-11 max-md:px-2"
                >
                  {opt.label}
                </SegmentedControlItem>
              ))}
            </SegmentedControl>
          </div>

          {pullChanges.length === 0 ? (
            <div className="rounded-sm border border-hairline bg-surface px-3 py-2 text-xs text-ink-2">
              Studio and local project files are in sync. No pull changes detected.
            </div>
          ) : (
            <>
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex flex-wrap items-center gap-1.5">
                  <Button
                    variant="outline"
                    size="xs"
                    data-pull-select-all
                    onClick={() => {
                      const all: Record<string, boolean> = {};
                      for (const c of pullChanges) all[c.path] = true;
                      setSelectedPaths(all);
                    }}
                    className="bg-surface max-md:min-h-11"
                  >
                    Select All
                  </Button>
                  <Button
                    variant="outline"
                    size="xs"
                    data-pull-select-clean
                    onClick={() => applySelectionFromChanges(pullChanges)}
                    className="bg-surface max-md:min-h-11"
                  >
                    Non-Conflicting
                  </Button>
                  <Button
                    variant="ghost"
                    size="xs"
                    data-pull-select-none
                    onClick={() => setSelectedPaths({})}
                    className="max-md:min-h-11"
                  >
                    Clear
                  </Button>
                </div>
                <span className="text-xs text-ink-2">
                  <span className="font-mono tabular-nums text-ink">{chosenPaths.length}</span> of{" "}
                  <span className="font-mono tabular-nums text-ink">{pullChanges.length}</span> selected
                </span>
              </div>

              <ul className="flex max-h-80 flex-col gap-2 overflow-y-auto" aria-label="Studio pull changes">
                {pullChanges.map((c) => {
                  const meta = PULL_STATUS_META[c.status] || { label: c.status, cls: "bg-board text-ink-2" };
                  const open = expandedPullPath === c.path;
                  const checked = Boolean(selectedPaths[c.path]);
                  const studioHex = formatShortHash(c.studio_hash);
                  const localHex = formatShortHash(c.local_hash);
                  return (
                    <li
                      key={`${c.kind}:${c.path}`}
                      data-pull-change-row={c.path}
                      className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <label className="flex min-w-0 flex-1 cursor-pointer items-center gap-2">
                          <input
                            type="checkbox"
                            checked={checked}
                            data-pull-checkbox={c.path}
                            aria-label={`Select ${c.path}`}
                            onChange={(e) =>
                              setSelectedPaths((prev) => ({ ...prev, [c.path]: e.target.checked }))
                            }
                            className="size-4 shrink-0 accent-blue"
                          />
                          <span className="min-w-0 truncate font-mono text-xs font-medium text-ink">{c.path}</span>
                        </label>
                        <div className="flex flex-wrap items-center gap-1.5">
                          <Pill className="bg-board px-1.5 py-0.5 text-xs text-ink-2">{c.kind}</Pill>
                          <Pill data-pull-status={c.status} className={`px-1.5 py-0.5 text-xs ${meta.cls}`}>
                            {meta.label}
                          </Pill>
                          <Button
                            variant="outline"
                            size="xs"
                            data-pull-preview-diff={c.path}
                            onClick={() => setExpandedPullPath(open ? null : c.path)}
                            className="max-md:min-h-11"
                          >
                            {open ? "Hide Diff" : "Preview Diff"}
                          </Button>
                        </div>
                      </div>

                      {(studioHex || localHex) && (
                        <div className="flex flex-wrap gap-3 pl-6 text-xs text-ink-3">
                          {localHex && (
                            <span>
                              local <span className="font-mono text-ink-2">{localHex}</span>
                            </span>
                          )}
                          {studioHex && (
                            <span>
                              studio <span className="font-mono text-ink-2">{studioHex}</span>
                            </span>
                          )}
                        </div>
                      )}

                      {open && (
                        <div
                          data-pull-hunks={c.path}
                          className="rounded-sm border border-hairline bg-board/40 p-2 font-mono text-xs"
                        >
                          {c.hunks.length > 0 ? (
                            c.hunks.slice(0, 4).map((h, hi) => renderPullHunk(h, hi))
                          ) : (
                            <div className="text-ink-2">
                              {c.status === "studio_only"
                                ? "New item authored in Roblox Studio."
                                : c.status === "local_only"
                                  ? "Item present only in your local Rojo project."
                                  : "Hashes differ between Studio and local project."}
                            </div>
                          )}
                        </div>
                      )}
                    </li>
                  );
                })}
              </ul>

              <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline pt-2">
                {needsOverwriteConfirm ? (
                  <label className="flex cursor-pointer items-center gap-2 text-xs text-pill-warn-fg">
                    <input
                      type="checkbox"
                      checked={confirmOverwrite}
                      data-pull-confirm-overwrite
                      onChange={(e) => setConfirmOverwrite(e.target.checked)}
                      className="size-4 accent-blue"
                    />
                    <span>Confirm overwrite of conflicting local files</span>
                  </label>
                ) : (
                  <span className="text-xs text-ink-2">
                    {strategy === "merge_non_conflicting"
                      ? "Applies Studio Only and Studio Modified items while keeping conflicts untouched."
                      : strategy === "take_studio"
                        ? "Writes selected Studio versions onto disk and updates sync baseline."
                        : "Keeps local files and advances sync baseline for selected paths."}
                  </span>
                )}
                <Button
                  variant="default"
                  size="sm"
                  loading={applyBusy}
                  disabled={chosenPaths.length === 0 || (needsOverwriteConfirm && !confirmOverwrite)}
                  onClick={() => void handlePullApply()}
                  data-pull-apply
                  className="max-md:min-h-11"
                >
                  <GitMerge className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Apply Selected ({chosenPaths.length})
                </Button>
              </div>
            </>
          )}
        </div>
      )}

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
                      <Pill className="bg-pill-warn-bg px-1.5 py-0.5 text-xs text-pill-warn-fg">
                        {c.status} +{c.added_lines}/-{c.removed_lines}L
                      </Pill>
                    </div>
                    <div className="mt-0.5 text-xs text-ink-2">
                      Place target: <code className="font-mono">{c.place_path}</code>
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
                        <div className="mb-1 font-mono text-xs font-medium text-ink-2">
                          Place Baseline ({c.place_path})
                        </div>
                        <pre className="max-h-36 overflow-auto font-mono text-xs leading-relaxed text-ink">
                          {placeLines.length ? placeLines.join("\n") : "-- (empty / not in baseline)"}
                        </pre>
                      </div>
                      <div className="rounded-sm border border-hairline bg-surface p-2">
                        <div className="mb-1 font-mono text-xs font-medium text-ink-2">
                          Local Workspace ({c.path})
                        </div>
                        <pre className="max-h-36 overflow-auto font-mono text-xs leading-relaxed text-ink">
                          {localLines.length ? localLines.join("\n") : "-- (deleted on disk)"}
                        </pre>
                      </div>
                    </div>
                    {c.hunks.length > 0 && (
                      <div className="rounded-sm border border-hairline bg-surface p-2 font-mono text-xs">
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
