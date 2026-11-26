import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Activity, AlertTriangle, Check, Code2, Eye, RotateCw, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  projUrl,
  type EconomySimPostResult,
  type EconomySimReport,
} from "./api";

export function EconomyBalanceSimCard({
  projectId,
  studioOk = true,
  hideWhenClean = false,
  onJumpToIssue,
  onApplied,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenClean?: boolean;
  onJumpToIssue?: (scriptPath: string, line?: number) => void;
  onApplied?: () => Promise<void> | void;
}) {
  const [report, setReport] = useState<EconomySimReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [applying, setApplying] = useState(false);
  const [showPreview, setShowPreview] = useState(false);

  const endpoint = projectId ? projUrl(projectId, "/economy-sim") : "/api/studio/economy-sim";

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const res = await call<EconomySimReport>(endpoint);
    setBusy(false);
    if (res.ok && typeof res.balanced === "boolean") {
      setReport(res as EconomySimReport);
    }
  }, [endpoint, projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const applySuggestions = async () => {
    if (!projectId && !studioOk) return;
    setApplying(true);
    const res = await call<EconomySimPostResult>(endpoint, { apply_suggestions: true });
    setApplying(false);
    if (!res.ok) {
      toast.error(res.error || "Could not apply balanced economy tuning.");
      return;
    }
    if (res.report) {
      setReport(res.report);
    } else {
      await load();
    }
    setShowPreview(false);
    await onApplied?.();
    toast.success(`Applied ${res.fixed_count ?? 0} economy balance adjustment(s).`);
  };

  if (hideWhenClean && (!report || report.balanced)) {
    return null;
  }

  const maxEarned = Math.max(
    1,
    ...(report?.curve ?? []).map((pt) => Math.max(pt.f2p_earned, pt.pass_earned)),
  );

  return (
    <section
      data-economy-balance-sim
      aria-label="Deterministic Economy Balance and Progression Simulator"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <Activity className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">
              Economy Balance &amp; 60m Progression
            </h3>
            {report && (
              <Pill
                className={
                  report.balanced
                    ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                    : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
              >
                {report.balanced ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>Balanced 60m curve</span>
                  </>
                ) : (
                  <span>{report.blocker_count} balance issue(s)</span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Simulates a deterministic 60-minute F2P and GamePass progression curve from your Luau earn, price, and rebirth constants.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            loading={busy}
            onClick={() => void load()}
            data-run-economy-balance-sim
            className="max-md:min-h-11"
          >
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Simulate 60m
          </Button>
          {report && !report.balanced && (
            <Button
              variant="outline"
              size="sm"
              onClick={() => setShowPreview((v) => !v)}
              data-preview-economy-suggestion
              className="max-md:min-h-11"
            >
              <Eye className="size-3.5" strokeWidth={1.5} aria-hidden />
              {showPreview ? "Hide Suggestion" : "Preview Suggestion"}
            </Button>
          )}
          <Button
            variant={report && !report.balanced ? "default" : "outline"}
            size="sm"
            loading={applying}
            disabled={!report || report.balanced || applying}
            onClick={() => void applySuggestions()}
            data-apply-economy-suggestion
            className="max-md:min-h-11"
          >
            <Wrench className="size-3.5" strokeWidth={1.5} aria-hidden />
            {applying ? "Tuning..." : "Apply Balanced Tuning"}
          </Button>
        </div>
      </div>

      {report && (
        <>
          <div className="grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
            <div className="rounded-sm border border-hairline bg-board/50 px-2.5 py-2">
              <div className="text-[11px] text-ink-2">Earn Rate</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink">
                {report.constants.earn_per_min.toFixed(0)}/min
              </div>
              <div className="font-mono text-[11px] tabular-nums text-ink-3">
                Rebirth {report.constants.rebirth_mult.toFixed(1)}x
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-2.5 py-2">
              <div className="text-[11px] text-ink-2">First Rebirth</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink">
                {report.summary.time_to_rebirth_min !== null
                  ? `${report.summary.time_to_rebirth_min} min`
                  : ">60 min"}
              </div>
              <div className="font-mono text-[11px] tabular-nums text-ink-3">
                Cost {report.constants.rebirth_cost.toFixed(0)}
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-2.5 py-2">
              <div className="text-[11px] text-ink-2">60m Completion</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink">
                {report.summary.f2p_completion_pct.toFixed(0)}% F2P
              </div>
              <div className="font-mono text-[11px] tabular-nums text-ink-3">
                {report.summary.pass_completion_pct.toFixed(0)}% GamePass
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-2.5 py-2">
              <div className="text-[11px] text-ink-2">Inflation Ratio</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink">
                {report.summary.inflation_ratio.toFixed(1)}x
              </div>
              <div className="font-mono text-[11px] tabular-nums text-ink-3">
                {report.summary.f2p_vs_pass_ratio.toFixed(1)}x Pass gap
              </div>
            </div>
          </div>

          <div
            className="flex flex-col gap-1.5 rounded-sm border border-hairline bg-board/40 px-3 py-2"
            data-economy-curve-bars
          >
            <div className="flex items-center justify-between text-[11px] text-ink-2">
              <span>60-minute progression checkpoints (F2P vs GamePass)</span>
              <span className="font-mono tabular-nums">
                {report.constants.tiers_count} tier(s)
              </span>
            </div>
            <div className="grid grid-cols-9 items-end gap-1 pt-1" aria-hidden>
              {report.curve.map((pt) => {
                const fPct = Math.max(6, Math.round((pt.f2p_earned / maxEarned) * 100));
                const pPct = Math.max(6, Math.round((pt.pass_earned / maxEarned) * 100));
                return (
                  <div key={pt.minute} className="flex flex-col items-center gap-1">
                    <div className="flex h-10 w-full items-end justify-center gap-0.5">
                      <div
                        className="w-2 rounded-t-xs bg-blue/80"
                        style={{ height: `${fPct}%` }}
                        title={`${pt.minute}m F2P: ${pt.f2p_earned}`}
                      />
                      <div
                        className="w-2 rounded-t-xs bg-ink-3/50"
                        style={{ height: `${pPct}%` }}
                        title={`${pt.minute}m Pass: ${pt.pass_earned}`}
                      />
                    </div>
                    <span className="font-mono text-[10px] tabular-nums text-ink-3">
                      {pt.minute}m
                    </span>
                  </div>
                );
              })}
            </div>
          </div>

          {showPreview && (
            <div
              data-economy-suggestion-preview
              className="rounded-sm border border-hairline bg-surface px-3 py-2 text-xs"
            >
              <div className="font-medium text-ink">1-Click Balance Suggestion Preview</div>
              <div className="mt-1 font-mono text-[11px] text-ink-2">
                {report.suggestion.diff_preview}
              </div>
              <div className="mt-1 text-ink-2">
                Projected: Rebirth at{" "}
                <span className="font-mono text-ink">
                  {report.suggestion.projected_summary.time_to_rebirth_min ?? 16}m
                </span>{" "}
                · Completion{" "}
                <span className="font-mono text-ink">
                  {report.suggestion.projected_summary.f2p_completion_pct.toFixed(0)}%
                </span>{" "}
                · Inflation{" "}
                <span className="font-mono text-ink">
                  {report.suggestion.projected_summary.inflation_ratio.toFixed(1)}x
                </span>
              </div>
            </div>
          )}

          {report.issues.length > 0 && (
            <ul className="flex flex-col gap-1.5 text-xs" aria-label="Economy balance warnings">
              {report.issues.map((issue) => (
                <li
                  key={issue.id}
                  data-economy-balance-issue={issue.id}
                  className="flex flex-wrap items-start justify-between gap-2 rounded-sm border border-hairline bg-pill-pro-bg/25 px-3 py-2"
                >
                  <div className="flex min-w-0 flex-1 items-start gap-2">
                    <AlertTriangle className="mt-0.5 size-3.5 shrink-0 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium text-ink">{issue.title}</span>
                        <Pill className="bg-pill-warn-bg px-1.5 py-0.5 text-[11px] text-pill-warn-fg">
                          {issue.category}
                        </Pill>
                      </div>
                      <div className="mt-0.5 text-ink-2">
                        <span className="font-mono text-ink">
                          {issue.file}:{issue.line}
                        </span>{" "}
                        · {issue.detail}
                      </div>
                    </div>
                  </div>
                  {onJumpToIssue && (
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => onJumpToIssue(issue.place_path || issue.file, issue.line)}
                      data-jump-to-economy-issue={issue.id}
                      className="max-md:min-h-11"
                    >
                      <Code2 className="size-3" strokeWidth={1.5} aria-hidden />
                      Jump to Issue
                    </Button>
                  )}
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </section>
  );
}
