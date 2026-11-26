import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, Code2, RotateCw, ShieldAlert, ShieldCheck, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import {
  call,
  projUrl,
  type PublishGuardAutofixResult,
  type PublishGuardIssue,
  type PublishGuardReport,
} from "./api";

export function PublishGuardCard({
  projectId,
  studioOk = true,
  hideWhenClean = false,
  onJumpToIssue,
  onFixed,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  hideWhenClean?: boolean;
  onJumpToIssue?: (scriptPath: string, line?: number) => void;
  onFixed?: () => Promise<void> | void;
}) {
  const [report, setReport] = useState<PublishGuardReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [fixing, setFixing] = useState<string | null>(null);

  const load = useCallback(async () => {
    if (!projectId && !studioOk) return;
    setBusy(true);
    const endpoint = projectId ? projUrl(projectId, "/publish-guard") : "/api/studio/publish-guard";
    const res = await call<PublishGuardReport>(endpoint);
    setBusy(false);
    if (res.ok && typeof res.ready_to_publish === "boolean") {
      setReport(res as PublishGuardReport);
    }
  }, [projectId, studioOk]);

  useEffect(() => {
    void load();
  }, [load]);

  const applyAutofix = async (fixAction?: string) => {
    if (!projectId && !studioOk) return;
    setFixing(fixAction || "all");
    const endpoint = projectId
      ? projUrl(projectId, "/publish-guard/autofix")
      : "/api/studio/publish-guard/autofix";
    const res = await call<PublishGuardAutofixResult>(
      endpoint,
      fixAction ? { fix_action: fixAction } : {},
    );
    setFixing(null);
    if (!res.ok) {
      toast.error(res.error || "Could not apply safe autofix.");
      return;
    }
    if (res.report) {
      setReport(res.report);
    } else {
      await load();
    }
    await onFixed?.();
    toast.success(`Applied ${res.fixed_count ?? 0} safe autofix(es).`);
  };

  if (hideWhenClean && (!report || report.ready_to_publish)) {
    return null;
  }

  const issues: PublishGuardIssue[] = report?.issues ?? [];
  const totalIssues = (report?.blocker_count ?? 0) + (report?.warning_count ?? 0);

  return (
    <section
      data-publish-guard
      aria-label="Pre-Publish Luau and Asset Readiness Guard"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            {report && !report.ready_to_publish ? (
              <ShieldAlert className="size-4 text-pill-pro-fg" strokeWidth={1.5} aria-hidden />
            ) : (
              <ShieldCheck className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            )}
            <h3 className="font-heading text-ui font-medium text-ink">
              Pre-Publish Luau &amp; Asset Readiness Guard
            </h3>
            {report && (
              <Pill
                className={
                  report.ready_to_publish
                    ? "bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg"
                    : "bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg"
                }
              >
                {report.ready_to_publish ? (
                  <>
                    <Check className="size-3" strokeWidth={2} aria-hidden />
                    <span>0 publish blockers</span>
                  </>
                ) : (
                  <span>
                    {report.blocker_count} blocker(s) · {report.warning_count} warning(s)
                  </span>
                )}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Scans scripts for orphan asset IDs, broken remotes, unsafe dynamic calls, and missing spawn or shop hooks before exporting to Roblox Studio.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="outline"
            size="sm"
            loading={busy}
            onClick={() => void load()}
            data-refresh-publish-guard
            className="max-md:min-h-11"
          >
            <RotateCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Scan
          </Button>
          <Button
            variant={totalIssues > 0 ? "default" : "outline"}
            size="sm"
            loading={fixing === "all"}
            disabled={!report || report.autofixable_count === 0 || Boolean(fixing)}
            onClick={() => void applyAutofix()}
            data-publish-guard-autofix
            className="max-md:min-h-11"
          >
            <Wrench className="size-3.5" strokeWidth={1.5} aria-hidden />
            {fixing === "all" ? "Fixing…" : "Apply Safe Autofix"}
          </Button>
        </div>
      </div>

      {report && (
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-4 text-xs">
          {(
            [
              ["assets", "Asset Refs"],
              ["remotes", "Remotes & Requires"],
              ["security", "Luau Security"],
              ["hooks", "Spawn & Shop"],
            ] as const
          ).map(([key, label]) => {
            const ok = report.checks?.[key] ?? true;
            return (
              <div
                key={key}
                className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-2.5 py-1.5"
              >
                <span className="text-ink-2">{label}</span>
                <span className={ok ? "font-mono text-pill-active-fg" : "font-mono text-pill-pro-fg"}>
                  {ok ? "OK" : "FIX"}
                </span>
              </div>
            );
          })}
        </div>
      )}

      {issues.length > 0 && (
        <ul className="flex flex-col gap-1.5 text-xs" aria-label="Pre-publish guard issues">
          {issues.map((issue) => (
            <li
              key={issue.id}
              data-publish-guard-issue={issue.id}
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
                    data-jump-to-issue={issue.id}
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
                    onClick={() => void applyAutofix(issue.fix_action)}
                    data-autofix-issue={issue.id}
                    className="max-md:min-h-11"
                  >
                    <Wrench className="size-3" strokeWidth={1.5} aria-hidden />
                    Fix
                  </Button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
