import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, RotateCw, ShieldAlert, ShieldCheck, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  enc,
  projUrl,
  type HookIntegrityReport,
  type HookRestoreResult,
} from "./api";

export function HookIntegrityCard({
  projectId,
  studioOk = true,
  hideWhenHealthy = false,
  onRestored,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenHealthy?: boolean;
  onRestored?: (runId?: string) => Promise<void> | void;
}) {
  const [report, setReport] = useState<HookIntegrityReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [restoring, setRestoring] = useState(false);

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const endpoint = projectId
      ? projUrl(projectId, "/snapshots/hook-integrity")
      : "/api/studio/snapshots/hook-integrity";
    const res = await call<HookIntegrityReport>(endpoint);
    setBusy(false);
    if (res.ok && typeof res.healthy === "boolean") {
      setReport(res as HookIntegrityReport);
    }
  }, [projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const restoreHooks = async () => {
    const sid = report?.baseline_snapshot_id;
    if ((!projectId && !studioOk) || !sid) return;
    setRestoring(true);
    const endpoint = projectId
      ? projUrl(projectId, `/snapshots/${enc(sid)}/restore-hooks`)
      : `/api/snapshots/${enc(sid)}/restore-hooks`;
    const res = await call<HookRestoreResult>(endpoint, { auto_checkpoint: true });
    setRestoring(false);
    if (!res.ok) {
      toast.error(res.error || "Could not restore missing hooks.");
      return;
    }
    if (res.after) {
      setReport(res.after);
    } else {
      await load();
    }
    await onRestored?.(res.rerun?.run_id);
    toast.success(
      `Surgically restored ${res.hooks_restored ?? 0} missing hook(s); preserved ${res.preserved_new_scripts?.length ?? 0} new script(s).`,
    );
  };

  if (hideWhenHealthy && (!report || !report.has_baseline || report.healthy)) {
    return null;
  }

  const missing = report?.missing_hooks ?? [];
  const preservedCount = report?.preserved_new_scripts?.length ?? 0;

  return (
    <section
      data-hook-integrity
      aria-label="Preflight Hook Integrity and Regression Diff Guard"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            {report && !report.healthy ? (
              <ShieldAlert className="size-4 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
            ) : (
              <ShieldCheck className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            )}
            <h3 className="font-heading text-ui font-medium text-ink">
              Preflight Hook-Integrity &amp; Regression Guard
            </h3>
            {report && (
              <Pill
                className={
                  !report.has_baseline
                    ? "bg-board px-2 py-0.5 text-xs text-ink-2"
                    : report.healthy
                      ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                      : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
              >
                {!report.has_baseline ? (
                  <span>Awaiting verified baseline</span>
                ) : report.healthy ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>All revenue &amp; core hooks intact</span>
                  </>
                ) : (
                  <span>{report.missing_count} missing hook(s)</span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Detects dropped Robux GamePass/Product IDs, <span className="font-mono">ProcessReceipt</span>,{" "}
            <span className="font-mono">Workspace.SpawnLocation</span>, and <span className="font-mono">--!strict</span> headers against{" "}
            {report?.baseline_label ? (
              <span className="font-medium text-ink">“{report.baseline_label}”</span>
            ) : (
              "your latest verified snapshot"
            )}
            , and restores missing hooks without discarding new feature scripts.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            loading={busy}
            onClick={() => void load()}
            data-refresh-hook-integrity
            className="max-md:min-h-11"
          >
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Refresh
          </Button>
          <Button
            variant={report && !report.healthy ? "default" : "outline"}
            size="sm"
            loading={restoring}
            disabled={!report?.baseline_snapshot_id || report.healthy || restoring}
            onClick={() => void restoreHooks()}
            data-restore-missing-hooks
            className="max-md:min-h-11"
          >
            <Wrench className="size-3.5" strokeWidth={1.5} aria-hidden />
            {restoring ? "Restoring Hooks…" : "Restore Missing Revenue / Core Hooks"}
          </Button>
        </div>
      </div>

      {report?.has_baseline && (
        <div className="flex flex-wrap items-center gap-2 text-xs text-ink-2" data-preserved-scripts>
          <span className="rounded-sm border border-hairline bg-board/50 px-2 py-0.5 font-mono text-[11px] tabular-nums">
            {preservedCount} new feature script(s) preserved on surgical merge
          </span>
        </div>
      )}

      {missing.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" aria-label="Missing revenue or core hooks">
          {missing.map((issue) => (
            <li
              key={issue.id}
              data-missing-hook-row={issue.id}
              className="flex items-start gap-2 rounded-sm border border-hairline bg-pill-pro-bg/30 px-3 py-2"
            >
              <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
              <div className="min-w-0 flex-1">
                <div className="flex flex-wrap items-center gap-2">
                  <span className="font-medium text-ink">{issue.label}</span>
                  <Pill className="bg-pill-warn-bg px-1.5 py-0.5 text-[11px] text-pill-warn-fg">
                    {issue.category}
                  </Pill>
                </div>
                <div className="mt-0.5 text-ink-2">
                  <span className="font-mono text-ink">{issue.file}</span> · {issue.detail}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
