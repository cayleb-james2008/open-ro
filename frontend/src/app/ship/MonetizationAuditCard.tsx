import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, Code2, RotateCw, ShieldAlert, ShieldCheck, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  projUrl,
  type MonetizationAuditIssue,
  type MonetizationAuditRepairResult,
  type MonetizationAuditReport,
} from "./api";

export function MonetizationAuditCard({
  projectId,
  studioOk = true,
  hideWhenClean = false,
  onJumpToIssue,
  onRepaired,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenClean?: boolean;
  onJumpToIssue?: (scriptPath: string, line?: number) => void;
  onRepaired?: () => Promise<void> | void;
}) {
  const [report, setReport] = useState<MonetizationAuditReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [fixing, setFixing] = useState<string | null>(null);

  const endpoint = projectId ? projUrl(projectId, "/monetization-audit") : "/api/studio/monetization-audit";

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const res = await call<MonetizationAuditReport>(endpoint);
    setBusy(false);
    if (res.ok && typeof res.ready === "boolean") {
      setReport(res as MonetizationAuditReport);
    }
  }, [endpoint, projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const applyRepair = async (fixAction?: string) => {
    if (!projectId && !studioOk) return;
    setFixing(fixAction || "all");
    const res = await call<MonetizationAuditRepairResult>(
      endpoint,
      fixAction ? { fix_action: fixAction } : {},
    );
    setFixing(null);
    if (!res.ok) {
      toast.error(res.error || "Could not apply safe monetization repair.");
      return;
    }
    if (res.report) {
      setReport(res.report);
    } else {
      await load();
    }
    await onRepaired?.();
    toast.success(`Applied ${res.fixed_count ?? 0} monetization safety repair(s).`);
  };

  if (hideWhenClean && (!report || report.ready)) {
    return null;
  }

  const issues: MonetizationAuditIssue[] = report?.issues ?? [];
  const totalIssues = (report?.blocker_count ?? 0) + (report?.warning_count ?? 0);

  return (
    <section
      data-monetization-audit
      aria-label="Monetization and Retention Safety Audit"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            {report && !report.ready ? (
              <ShieldAlert className="size-4 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
            ) : (
              <ShieldCheck className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            )}
            <h3 className="font-heading text-ui font-medium text-ink">
              Monetization &amp; Retention Safety Audit
            </h3>
            {report && (
              <Pill
                className={
                  report.ready
                    ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                    : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
              >
                {report.ready ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>Purchase &amp; retention safe</span>
                  </>
                ) : (
                  <span>
                    {report.blocker_count} issue(s) · {report.warning_count} note(s)
                  </span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Verifies idempotent ProcessReceipt PurchaseId ledgers, DataStore BindToClose flushes, pcall-wrapped GamePass prompts, and first-60s player onboarding hooks.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            loading={busy}
            onClick={() => void load()}
            data-refresh-monetization-audit
            className="max-md:min-h-11"
          >
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Audit
          </Button>
          <Button
            variant={totalIssues > 0 ? "default" : "outline"}
            size="sm"
            loading={fixing === "all"}
            disabled={!report || report.autofixable_count === 0 || Boolean(fixing)}
            onClick={() => void applyRepair()}
            data-monetization-audit-repair
            className="max-md:min-h-11"
          >
            <Wrench className="size-3.5" strokeWidth={1.5} aria-hidden />
            {fixing === "all" ? "Repairing..." : "Apply Safe Repair"}
          </Button>
        </div>
      </div>

      {report && (
        <div className="grid grid-cols-2 gap-2 text-xs sm:grid-cols-4">
          {(
            [
              ["process_receipt", "ProcessReceipt"],
              ["bind_to_close", "BindToClose Flush"],
              ["gamepass_pcall", "GamePass pcall"],
              ["onboarding_retention", "60s Onboarding"],
            ] as const
          ).map(([key, label]) => {
            const ok = report.checks?.[key] ?? true;
            return (
              <div
                key={key}
                className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-2.5 py-1.5"
              >
                <span className="truncate text-ink-2">{label}</span>
                <span className={ok ? "font-mono text-pill-active-fg" : "font-mono text-pill-pro-fg"}>
                  {ok ? "OK" : "FIX"}
                </span>
              </div>
            );
          })}
        </div>
      )}

      {issues.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" aria-label="Monetization safety findings">
          {issues.map((issue) => (
            <li
              key={issue.id}
              data-monetization-audit-issue={issue.id}
              className="flex flex-col gap-2 rounded-sm border border-hairline bg-pill-pro-bg/25 px-3 py-2"
            >
              <div className="flex flex-wrap items-start justify-between gap-2">
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
                      · {issue.fix_hint}
                    </div>
                  </div>
                </div>
                <div className="flex shrink-0 items-center gap-1.5">
                  {onJumpToIssue && (
                    <Button
                      variant="outline"
                      size="xs"
                      onClick={() => onJumpToIssue(issue.place_path || issue.file, issue.line)}
                      data-jump-to-monetization-issue={issue.id}
                      className="max-md:min-h-11"
                    >
                      <Code2 className="size-3" strokeWidth={1.5} aria-hidden />
                      Jump to Issue
                    </Button>
                  )}
                  {issue.autofixable && (
                    <Button
                      variant="outline"
                      size="xs"
                      loading={fixing === issue.fix_action}
                      disabled={Boolean(fixing)}
                      onClick={() => void applyRepair(issue.fix_action)}
                      data-repair-monetization-issue={issue.id}
                      className="max-md:min-h-11"
                    >
                      <Wrench className="size-3" strokeWidth={1.5} aria-hidden />
                      Repair
                    </Button>
                  )}
                </div>
              </div>
              {issue.snippet && (
                <pre className="max-h-24 overflow-auto rounded-sm border border-hairline bg-board/80 px-2.5 py-1.5 font-mono text-[11px] text-ink-2">
                  {issue.snippet}
                </pre>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
