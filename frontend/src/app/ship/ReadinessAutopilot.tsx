import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, Coins, Rocket, Sparkles, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { call, projUrl, type AutopilotResult, type MonetizationCodegenResult, type ProjectReadiness } from "./api";

/**
 * 1-click Ship & Monetization Readiness Autopilot:
 * audits Luau prover + Rojo build + Asset Lock + Monetization/Retention + Open Cloud
 * and runs safe auto-fixes (format, empty stubs, AssetIds/ProductIds codegen, Monetization Pack, .rbxlx build).
 */
export function ReadinessAutopilot({
  projectId,
  projectName,
  onCompleted,
  onOpenCodeFile,
  onOpenSettings,
}: {
  projectId: string;
  projectName: string;
  onCompleted?: () => Promise<void> | void;
  onOpenCodeFile?: (scriptPath: string) => void;
  onOpenSettings?: () => void;
}) {
  const [readiness, setReadiness] = useState<ProjectReadiness | null>(null);
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [codegenBusy, setCodegenBusy] = useState(false);
  const [lastResult, setLastResult] = useState<AutopilotResult | null>(null);
  const [codegenInfo, setCodegenInfo] = useState<string>("");
  const [error, setError] = useState("");

  const loadReadiness = useCallback(async () => {
    setLoading(true);
    setError("");
    const r = await call<ProjectReadiness>(projUrl(projectId, "/readiness"));
    setLoading(false);
    if (!r.ok || !Array.isArray(r.checks)) {
      setError(r.error || "Could not evaluate Ship readiness.");
      return;
    }
    setReadiness(r as ProjectReadiness);
  }, [projectId]);

  useEffect(() => {
    setLastResult(null);
    setCodegenInfo("");
    void loadReadiness();
  }, [loadReadiness]);

  const runEconomyCodegen = async () => {
    setCodegenBusy(true);
    setError("");
    const r = await call<MonetizationCodegenResult>(projUrl(projectId, "/monetization/codegen"), {
      seed_defaults: true,
    });
    setCodegenBusy(false);
    if (!r.ok) {
      setError(r.error || "Economy codegen could not finish.");
      toast.error(r.error || "Economy codegen failed");
      return;
    }
    if (r.readiness && Array.isArray(r.readiness.checks)) {
      setReadiness(r.readiness);
    } else {
      await loadReadiness();
    }
    const count = r.written?.length ?? 0;
    const summary = `Generated ${count} Luau monetization/economy script(s) (${r.luau_errors ?? 0} errors)${r.seeded_products ? " and seeded starter Pass/Product drafts" : ""}`;
    setCodegenInfo(summary);
    toast.success(`Economy & Shop Luau generated (${r.luau_errors ?? 0} errors)`);
    await onCompleted?.();
  };

  const runAutopilot = async () => {
    setRunning(true);
    setError("");
    const r = await call<AutopilotResult>(projUrl(projectId, "/autopilot"), {
      install_monetization: true,
      format: true,
      build: true,
      fix_empty: true,
    });
    setRunning(false);
    if (!r.ok) {
      setError(r.error || "Ship Autopilot could not finish.");
      toast.error(r.error || "Ship Autopilot failed");
      return;
    }
    setLastResult(r as AutopilotResult);
    if (r.readiness && Array.isArray(r.readiness.checks)) {
      setReadiness(r.readiness);
    } else {
      await loadReadiness();
    }
    const fixedCount = r.actions_taken?.length ?? 0;
    toast.success(fixedCount > 0 ? `Autopilot completed ${fixedCount} step(s)` : `${projectName} readiness verified`);
    await onCompleted?.();
  };

  const score = readiness?.score_pct ?? 0;
  const passedCount = readiness?.passed_count ?? 0;
  const totalCount = readiness?.total_count ?? 7;
  const fixableCount = readiness?.auto_fixable_count ?? 0;
  const diags = readiness?.diagnostics ?? [];

  return (
    <section
      data-ship-autopilot
      aria-label="Ship and Monetization Readiness Autopilot"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProgressCircle value={score} radius={16} strokeWidth={3.5} className="size-10 flex-none">
            <span className="font-mono text-xs font-medium tabular-nums text-ink">{score}%</span>
          </ProgressCircle>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-heading text-ui font-medium text-ink">Ship &amp; Monetization Autopilot</h3>
              <Pill className={readiness?.ready_to_publish ? "bg-pill-active-bg text-pill-active-fg" : "bg-board text-ink-2"}>
                <span className="font-mono tabular-nums">{passedCount}/{totalCount}</span>
                <span>{readiness?.ready_to_publish ? "Ship ready" : "Checks"}</span>
              </Pill>
            </div>
            <p className="mt-0.5 text-sm text-ink-2">
              {fixableCount > 0
                ? `${fixableCount} auto-fixable step(s): Luau hygiene, formatting, monetization/retention wiring, codegen, and .rbxlx build.`
                : readiness?.ready_to_publish
                  ? "All Luau, build, monetization, and Open Cloud gates are green."
                  : "All automatable checks passed. Complete the remaining owner-approved steps below."}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="default"
            size="sm"
            loading={running}
            disabled={codegenBusy}
            onClick={() => void runAutopilot()}
            data-run-autopilot
          >
            <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
            {fixableCount > 0 ? "Auto-Fix & Prepare to Ship" : "Re-Verify & Build"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            loading={codegenBusy}
            disabled={running}
            onClick={() => void runEconomyCodegen()}
            data-run-economy-codegen
          >
            <Coins className="size-3.5" strokeWidth={1.5} aria-hidden />
            Generate Shop &amp; Economy
          </Button>
          {onOpenSettings && readiness?.checks?.some((c) => c.id === "opencloud_auth" && !c.ok) && (
            <Button variant="outline" size="sm" onClick={onOpenSettings}>
              <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />
              Link Open Cloud
            </Button>
          )}
        </div>
      </div>

      {error && (
        <div role="alert" className="flex items-center justify-between gap-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
          <span>{error}</span>
          <Button variant="ghost" size="xs" onClick={() => void loadReadiness()}>Retry</Button>
        </div>
      )}

      {loading && !readiness ? (
        <p className="text-sm text-ink-2">Checking Luau scripts, assets, monetization, and build status…</p>
      ) : readiness && (
        <ul className="grid gap-1.5 sm:grid-cols-2" aria-label="Unified ship readiness checklist">
          {readiness.checks.map((c) => (
            <li
              key={c.id}
              className="flex items-start gap-2 rounded-lg border border-hairline bg-board/50 px-2.5 py-2 text-xs"
            >
              <span
                className={`mt-0.5 grid size-4 shrink-0 place-items-center rounded-full ${
                  c.ok
                    ? "bg-pill-active-bg text-pill-active-fg"
                    : c.auto_fixable
                      ? "bg-pill-warn-bg text-pill-warn-fg"
                      : "bg-track text-ink-3"
                }`}
              >
                {c.ok ? (
                  <Check className="size-3" strokeWidth={2} aria-hidden />
                ) : c.auto_fixable ? (
                  <Wrench className="size-2.5" strokeWidth={1.75} aria-hidden />
                ) : (
                  <AlertTriangle className="size-2.5" strokeWidth={1.75} aria-hidden />
                )}
              </span>
              <div className="min-w-0 flex-1">
                <div className="flex items-center justify-between gap-1">
                  <span className="font-medium text-ink">{c.label}</span>
                  {!c.ok && c.auto_fixable && (
                    <span className="shrink-0 rounded-chip bg-pill-warn-bg px-1.5 py-0 text-xs text-pill-warn-fg">
                      Auto-fix
                    </span>
                  )}
                </div>
                <div className="text-ink-2">{c.detail}</div>
              </div>
            </li>
          ))}
        </ul>
      )}

      {diags.length > 0 && (
        <div className="flex flex-col gap-1.5 rounded-sm border border-hairline bg-board/50 p-2.5 text-xs">
          <div className="font-medium text-ink">Top Luau diagnostics ({diags.length})</div>
          <ul className="flex flex-col gap-1">
            {diags.slice(0, 4).map((d, idx) => (
              <li key={`${d.path}:${d.line}:${idx}`} className="flex items-center justify-between gap-2">
                <span className="min-w-0 truncate font-mono text-ink-2">
                  {d.path}:{d.line} · {d.message}
                </span>
                {onOpenCodeFile && (
                  <button
                    type="button"
                    onClick={() => onOpenCodeFile(d.path)}
                    className="shrink-0 cursor-pointer font-medium text-blue hover:underline"
                  >
                    Inspect
                  </button>
                )}
              </li>
            ))}
          </ul>
        </div>
      )}

      {codegenInfo && (
        <div className="rounded-sm border border-hairline bg-board/40 px-3 py-2 text-xs text-pill-active-fg" data-economy-codegen-summary>
          {codegenInfo}
        </div>
      )}

      {lastResult && (lastResult.actions_taken?.length > 0 || lastResult.remaining_manual?.length > 0) && (
        <div className="flex flex-col gap-1 rounded-sm border border-hairline bg-board/40 px-3 py-2 text-xs" data-autopilot-summary>
          {lastResult.actions_taken?.length > 0 && (
            <div className="text-pill-active-fg">
              Completed: {lastResult.actions_taken.map((a) => a.action.replace(/_/g, " ")).join(" · ")}
            </div>
          )}
          {lastResult.remaining_manual?.map((m) => (
            <div key={m} className="text-ink-2">Next manual step: {m}</div>
          ))}
        </div>
      )}
    </section>
  );
}
