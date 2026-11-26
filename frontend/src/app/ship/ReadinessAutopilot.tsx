import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, Coins, Rocket, ShieldCheck, Sparkles, Wrench } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { call, projUrl, type AutopilotResult, type MonetizationCodegenResult, type ProjectReadiness, type PublishGateResult } from "./api";
import { HookIntegrityCard } from "./HookIntegrityCard";
import { SyncConflictsCard } from "./SyncConflictsCard";

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
  onOpenSection,
}: {
  projectId: string;
  projectName: string;
  onCompleted?: () => Promise<void> | void;
  onOpenCodeFile?: (scriptPath: string) => void;
  onOpenSettings?: () => void;
  onOpenSection?: (section: "publish" | "assets" | "art" | "playtests") => void;
}) {
  const [readiness, setReadiness] = useState<ProjectReadiness | null>(null);
  const [loading, setLoading] = useState(true);
  const [running, setRunning] = useState(false);
  const [fixingAction, setFixingAction] = useState<string | null>(null);
  const [codegenBusy, setCodegenBusy] = useState(false);
  const [gateBusy, setGateBusy] = useState(false);
  const [gateResult, setGateResult] = useState<PublishGateResult | null>(null);
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
    setGateResult(null);
    setCodegenInfo("");
    void loadReadiness();
  }, [loadReadiness]);

  const runPublishGate = async (silentToast = false) => {
    setGateBusy(true);
    setError("");
    const r = await call<PublishGateResult>(projUrl(projectId, "/publish-gate"), {});
    setGateBusy(false);
    if (!r.ok) {
      setError(r.error || "Pre-publish gate check failed.");
      toast.error(r.error || "Pre-publish gate failed");
      return;
    }
    setGateResult(r as PublishGateResult);
    if (r.readiness && Array.isArray(r.readiness.checks)) {
      setReadiness(r.readiness);
    }
    if (!silentToast) {
      if (r.gate_passed) {
        toast.success("Pre-Publish Revenue & Playtest Gate passed");
      } else {
        toast.info(`Gate found ${r.fix_cards?.length ?? 0} auto-fixable item(s)`);
      }
    }
  };

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

  const runAutopilot = async (singleAction?: string) => {
    if (singleAction) setFixingAction(singleAction);
    else setRunning(true);
    setError("");
    const r = await call<AutopilotResult>(
      projUrl(projectId, "/autopilot"),
      singleAction
        ? { action: singleAction }
        : {
            install_monetization: true,
            format: true,
            build: true,
            fix_empty: true,
          },
    );
    if (singleAction) setFixingAction(null);
    else setRunning(false);
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
    toast.success(
      singleAction
        ? `Fixed ${singleAction.replace(/_/g, " ")}`
        : fixedCount > 0
          ? `Autopilot completed ${fixedCount} step(s)`
          : `${projectName} readiness verified`,
    );
    if (gateResult) {
      await runPublishGate(true);
    }
    await onCompleted?.();
  };

  const score = readiness?.score_pct ?? 0;
  const passedCount = readiness?.passed_count ?? 0;
  const totalCount = readiness?.total_count ?? 8;
  const fixableCount = readiness?.auto_fixable_count ?? 0;
  const diags = readiness?.diagnostics ?? [];
  const pillars = readiness?.pillars ? Object.entries(readiness.pillars) : [];
  const blockers = readiness?.blockers ?? readiness?.checks?.filter((c) => !c.ok) ?? [];

  return (
    <section
      data-ship-autopilot
      aria-label="Game Quality and Revenue Readiness"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProgressCircle value={score} radius={18} strokeWidth={3.5} className="size-11 flex-none" data-readiness-score>
            <span className="font-mono text-xs font-medium tabular-nums text-ink">{score}</span>
          </ProgressCircle>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-2">
              <h3 className="font-heading text-ui font-medium text-ink">Quality &amp; Revenue Readiness</h3>
              <Pill className={readiness?.ready_to_publish ? "bg-pill-active-bg text-pill-active-fg" : "bg-board text-ink-2"}>
                <span className="font-mono tabular-nums">{passedCount}/{totalCount}</span>
                <span>{readiness?.ready_to_publish ? "Ship ready" : `${score}/100`}</span>
              </Pill>
              {lastResult && typeof lastResult.before_score === "number" && typeof lastResult.after_score === "number" && (
                <Pill
                  data-score-delta
                  className={
                    (lastResult.score_delta ?? 0) > 0
                      ? "bg-pill-active-bg text-pill-active-fg"
                      : "bg-board text-ink-2"
                  }
                >
                  <span className="font-mono tabular-nums">
                    {lastResult.before_score} → {lastResult.after_score} ({(lastResult.score_delta ?? 0) >= 0 ? `+${lastResult.score_delta ?? 0}` : lastResult.score_delta} pts)
                  </span>
                </Pill>
              )}
            </div>
            <p className="mt-0.5 text-sm text-ink-2">
              {blockers.length === 0
                ? "All Luau, playtest, monetization, asset, and Open Cloud gates are green."
                : fixableCount > 0
                  ? `${blockers.length} blocker(s) surfaced below — ${fixableCount} auto-fixable in 1 click.`
                  : `${blockers.length} manual gate(s) remaining before live publish.`}
            </p>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="default"
            size="sm"
            loading={running}
            disabled={codegenBusy || Boolean(fixingAction)}
            onClick={() => void runAutopilot()}
            data-run-autopilot
          >
            <Sparkles className="size-3.5" strokeWidth={1.5} aria-hidden />
            {fixableCount > 0 ? "Fix All Blockers & Re-Verify" : "Re-Verify & Build"}
          </Button>
          <Button
            variant="outline"
            size="sm"
            loading={codegenBusy}
            disabled={running || gateBusy || Boolean(fixingAction)}
            onClick={() => void runEconomyCodegen()}
            data-run-economy-codegen
          >
            <Coins className="size-3.5" strokeWidth={1.5} aria-hidden />
            Generate Shop &amp; Economy
          </Button>
          <Button
            variant="outline"
            size="sm"
            loading={gateBusy}
            disabled={running || codegenBusy || Boolean(fixingAction)}
            onClick={() => void runPublishGate()}
            data-run-publish-gate
          >
            <ShieldCheck className="size-3.5" strokeWidth={1.5} aria-hidden />
            Pre-Publish Gate
          </Button>
          {onOpenSettings && readiness?.checks?.some((c) => c.id === "opencloud_auth" && !c.ok) && (
            <Button variant="outline" size="sm" onClick={onOpenSettings}>
              <Rocket className="size-3.5" strokeWidth={1.5} aria-hidden />
              Link Open Cloud
            </Button>
          )}
        </div>
      </div>

      {pillars.length > 0 && (
        <div className="grid grid-cols-2 gap-2 sm:grid-cols-5" data-readiness-pillars>
          {pillars.map(([k, p]) => {
            const full = p.earned >= p.max;
            const d = lastResult?.pillar_deltas?.[k]?.delta ?? 0;
            return (
              <div key={k} className="flex items-center justify-between gap-1.5 rounded-lg border border-hairline bg-board/50 px-2.5 py-1.5 text-xs">
                <span className="truncate text-ink-2">{p.label}</span>
                <span className={`font-mono font-medium tabular-nums ${full ? "text-pill-active-fg" : "text-ink"}`}>
                  {p.earned}/{p.max}
                  {d > 0 && <span className="ml-1 text-pill-active-fg">+{d}</span>}
                </span>
              </div>
            );
          })}
        </div>
      )}

      {readiness && (readiness.ship_pack_integrity || readiness.monetization_parity) && (
        <div
          data-ship-pack-diagnostics
          className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-hairline bg-board/40 px-3 py-2 text-xs"
        >
          {readiness.ship_pack_integrity && (
            <div className="flex items-center gap-1.5">
              <span className={`font-medium ${readiness.ship_pack_integrity.ok ? "text-pill-active-fg" : "text-pill-pro-fg"}`}>
                Ship Pack ZIP: {readiness.ship_pack_integrity.ok ? "CRC-32 & zip-slip verified" : readiness.ship_pack_integrity.error || "Check failed"}
              </span>
              <span className="font-mono tabular-nums text-ink-3">
                ({readiness.ship_pack_integrity.entry_count} files)
              </span>
            </div>
          )}
          {readiness.monetization_parity && (
            <div className="flex items-center gap-1.5">
              <span className={`font-medium ${readiness.monetization_parity.ok ? "text-pill-active-fg" : "text-ink-2"}`}>
                ProductIds parity: {readiness.monetization_parity.missing_keys.length === 0 ? "Synced" : `${readiness.monetization_parity.missing_keys.length} missing`}
              </span>
              <span className="font-mono tabular-nums text-ink-3">
                ({readiness.monetization_parity.unapproved_prices === 0 ? "Price-gate OK" : `${readiness.monetization_parity.unapproved_prices} awaiting owner approval`})
              </span>
            </div>
          )}
        </div>
      )}

      {error && (
        <div role="alert" className="flex items-center justify-between gap-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">
          <span>{error}</span>
          <Button variant="ghost" size="xs" onClick={() => void loadReadiness()}>Retry</Button>
        </div>
      )}

      {loading && !readiness ? (
        <p className="text-sm text-ink-2">Scoring Luau proof, playtests, monetization, assets, and build status…</p>
      ) : readiness && (
        <ul className="grid gap-1.5 sm:grid-cols-2" aria-label="Unified ship readiness checklist">
          {readiness.checks.map((c) => (
            <li
              key={c.id}
              data-readiness-check={c.id}
              className="flex items-center justify-between gap-2 rounded-lg border border-hairline bg-board/50 px-2.5 py-2 text-xs"
            >
              <div className="flex min-w-0 flex-1 items-start gap-2">
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
                  <div className="flex items-center gap-1.5">
                    <span className="truncate font-medium text-ink">{c.label}</span>
                    {typeof c.weight === "number" && (
                      <span className="shrink-0 font-mono tabular-nums text-ink-3">
                        {c.earned ?? 0}/{c.weight}
                      </span>
                    )}
                  </div>
                  <div className="truncate text-ink-2">{c.detail}</div>
                </div>
              </div>
              {!c.ok && c.auto_fixable && c.fix_action && (
                <Button
                  variant="outline"
                  size="xs"
                  loading={fixingAction === c.fix_action}
                  disabled={running || codegenBusy}
                  onClick={() => void runAutopilot(c.fix_action ?? undefined)}
                  data-fix-action={c.fix_action}
                  className="shrink-0"
                >
                  Auto-Fix
                </Button>
              )}
              {!c.ok && !c.auto_fixable && c.cta_section && onOpenSection && (
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={() => onOpenSection(c.cta_section!)}
                  className="shrink-0"
                >
                  {c.cta_section === "playtests" ? "Playtest" : "Review"}
                </Button>
              )}
              {!c.ok && !c.auto_fixable && c.cta_action === "open_settings" && onOpenSettings && (
                <Button
                  variant="ghost"
                  size="xs"
                  onClick={onOpenSettings}
                  className="shrink-0"
                >
                  Settings
                </Button>
              )}
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
                  <Button
                    variant="ghost"
                    size="xs"
                    onClick={() => onOpenCodeFile(d.path)}
                    className="shrink-0"
                  >
                    Inspect
                  </Button>
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
              Completed: {lastResult.actions_taken.map((a) => a.action.replace(/_/g, " ")).join(", ")}
            </div>
          )}
          {lastResult.remaining_manual?.map((m) => (
            <div key={m} className="text-ink-2">Next manual step: {m}</div>
          ))}
        </div>
      )}

      <SyncConflictsCard
        projectId={projectId}
        hideWhenInSync
        onResolved={async () => {
          await loadReadiness();
          await onCompleted?.();
        }}
      />

      <HookIntegrityCard
        projectId={projectId}
        hideWhenHealthy
        onRestored={async () => {
          await loadReadiness();
          await onCompleted?.();
        }}
      />

      {gateResult && (
        <div className="flex flex-col gap-2 rounded-lg border border-hairline bg-board/40 p-3 text-xs" data-publish-gate-panel>
          <div className="flex flex-wrap items-center justify-between gap-2">
            <span className={`font-medium ${gateResult.gate_passed ? "text-pill-active-fg" : "text-ink"}`}>
              Pre-Publish Revenue &amp; Playtest Gate: {gateResult.gate_passed ? "Ready to Publish" : "Action Needed"}
            </span>
            <Pill className={gateResult.gate_passed ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-warn-bg text-pill-warn-fg"}>
              <span>{gateResult.checks.filter((c) => c.ok).length}/{gateResult.checks.length} passed</span>
            </Pill>
          </div>
          <ul className="grid gap-1.5 sm:grid-cols-2">
            {gateResult.checks.map((gc) => (
              <li key={gc.id} className="flex items-start gap-2 rounded-sm border border-hairline bg-surface/60 px-2 py-1.5">
                <span className={`mt-0.5 font-mono ${gc.ok ? "text-pill-active-fg" : "text-ink-3"}`}>
                  {gc.ok ? "OK" : "--"}
                </span>
                <div className="min-w-0">
                  <div className="font-medium text-ink">{gc.label}</div>
                  <div className="text-ink-2">{gc.detail}</div>
                </div>
              </li>
            ))}
          </ul>
          {gateResult.fix_cards?.length > 0 && (
            <div className="grid gap-1.5 sm:grid-cols-2">
              {gateResult.fix_cards.map((fc) => (
                <div
                  key={fc.id}
                  data-gate-fix-card={fc.id}
                  className="flex items-center justify-between gap-2 rounded-sm border border-hairline bg-surface px-2.5 py-2"
                >
                  <div className="min-w-0">
                    <div className="font-medium text-ink">{fc.title}</div>
                    <div className="truncate text-ink-2">{fc.reason}</div>
                  </div>
                  <Button
                    variant="default"
                    size="xs"
                    loading={fixingAction === fc.fix_action}
                    disabled={running || codegenBusy || gateBusy}
                    onClick={() => void runAutopilot(fc.fix_action)}
                    className="shrink-0"
                  >
                    Auto-Fix
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </section>
  );
}
