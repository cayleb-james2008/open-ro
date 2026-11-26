import { useState } from "react";
import { toast } from "sonner";
import { AlertTriangle, Check, Download, Play, TrendingUp } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Pill } from "@/components/kibo-ui/pill";
import { call, projUrl, type EconomyFunnelReport } from "./api";

const SIM_PRESETS = [
  { id: "", label: "Auto-detect" },
  { id: "starter_obby", label: "Obby / Sim" },
  { id: "anime_gacha_rpg", label: "Gacha / RPG" },
  { id: "competitive_arena", label: "Arena" },
] as const;

export function EconomySimulatorCard({
  projectId,
  studioOk = true,
  onTriggerFixAction,
}: {
  projectId?: string | null;
  studioOk?: boolean;
  onTriggerFixAction?: (fixAction: string) => void;
}) {
  const [preset, setPreset] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [report, setReport] = useState<EconomyFunnelReport | null>(null);

  const runDryRun = async () => {
    setBusy(true);
    const endpoint = projectId ? projUrl(projectId, "/monetization/simulate") : "/api/studio/economy/simulate";
    const res = await call<EconomyFunnelReport>(endpoint, preset ? { tier_preset: preset } : {});
    setBusy(false);
    if (!res.ok) {
      toast.error(res.error || "Economy & funnel dry-run failed.");
      return;
    }
    setReport(res as EconomyFunnelReport);
    toast.success(`Dry-run simulated: ${res.projections?.arpdau_robux?.toFixed(2) ?? "0"} R$ ARPDAU`);
  };

  const exportJson = () => {
    if (!report) return;
    const blob = new Blob([JSON.stringify(report, null, 2)], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "openro-economy-funnel.json";
    document.body.appendChild(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(url), 1000);
  };

  return (
    <section
      data-economy-simulator
      aria-label="Studio Economy and Funnel Dry-Run Simulator"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-2">
            <TrendingUp className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-medium text-ink">Economy &amp; Funnel Dry-Run Simulator</h3>
          </div>
          <p className="mt-0.5 text-xs text-ink-2">
            Deterministic ARPDAU, D1/D7 retention lift, and GamePass conversion projection from your place loop, starter items, and monetization wiring.
          </p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Button
            variant="default"
            size="sm"
            loading={busy}
            disabled={!projectId && !studioOk}
            onClick={() => void runDryRun()}
            data-run-economy-sim
            className="max-md:min-h-11"
          >
            <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
            {busy ? "Simulating…" : "Simulate Funnel"}
          </Button>
          {report && (
            <Button
              variant="outline"
              size="sm"
              onClick={exportJson}
              data-export-economy-json
              className="max-md:min-h-11"
            >
              <Download className="size-3.5" strokeWidth={1.5} aria-hidden />
              Export JSON
            </Button>
          )}
        </div>
      </div>

      <div className="flex flex-wrap items-center gap-1.5" role="group" aria-label="Simulation preset override">
        {SIM_PRESETS.map((p) => {
          const active = preset === p.id;
          return (
            <Button
              key={p.id || "auto"}
              type="button"
              variant={active ? "secondary" : "outline"}
              size="xs"
              aria-pressed={active}
              data-sim-preset={p.id || "auto"}
              onClick={() => setPreset(p.id)}
            >
              {p.label}
            </Button>
          );
        })}
      </div>

      {report && (
        <div className="flex flex-col gap-3" data-economy-sim-results>
          <div className="grid grid-cols-2 gap-2 sm:grid-cols-4">
            <div className="rounded-sm border border-hairline bg-board/50 px-3 py-2">
              <div className="text-[11px] text-ink-2">Projected ARPDAU</div>
              <div className="text-sm font-semibold tabular-nums text-ink" data-sim-arpdau>
                {report.projections.arpdau_robux.toFixed(2)} R$
              </div>
              <div className="text-[11px] tabular-nums text-ink-3">
                ~${report.projections.arpdau_usd_est.toFixed(3)} DevEx
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-3 py-2">
              <div className="text-[11px] text-ink-2">D1 Retention</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink" data-sim-d1>
                {report.projections.d1_retention_pct.toFixed(1)}%
              </div>
              <div className="text-[11px] tabular-nums text-pill-active-fg">
                +{report.projections.d1_lift_pct.toFixed(1)}% lift
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-3 py-2">
              <div className="text-[11px] text-ink-2">D7 Retention</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink" data-sim-d7>
                {report.projections.d7_retention_pct.toFixed(1)}%
              </div>
              <div className="text-[11px] tabular-nums text-pill-active-fg">
                +{report.projections.d7_lift_pct.toFixed(1)}% lift
              </div>
            </div>
            <div className="rounded-sm border border-hairline bg-board/50 px-3 py-2">
              <div className="text-[11px] text-ink-2">GamePass Conv.</div>
              <div className="font-mono text-sm font-semibold tabular-nums text-ink" data-sim-conv>
                {report.projections.gamepass_conversion_pct.toFixed(2)}%
              </div>
              <div className="text-[11px] tabular-nums text-ink-3">
                {report.place_signals.estimated_loop_minutes.toFixed(1)}m loop · {report.place_signals.starter_items_count} starter
              </div>
            </div>
          </div>

          <div className="grid gap-1.5 sm:grid-cols-4">
            {report.funnel_stages.map((st) => (
              <div key={st.stage} className="rounded-sm border border-hairline bg-surface px-2.5 py-2 text-xs">
                <div className="flex items-center justify-between gap-1">
                  <span className="truncate font-medium text-ink">{st.stage}</span>
                  <span className="font-mono tabular-nums text-ink">{st.rate_pct.toFixed(1)}%</span>
                </div>
                <div className="mt-0.5 truncate text-[11px] text-ink-2">{st.note}</div>
              </div>
            ))}
          </div>

          {report.blockers.length > 0 ? (
            <ul className="flex flex-col gap-1.5" aria-label="Actionable revenue blockers" data-sim-blockers>
              {report.blockers.map((b) => (
                <li
                  key={b.id}
                  className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-3 py-2 text-xs"
                >
                  <div className="flex min-w-0 flex-1 items-start gap-2">
                    <AlertTriangle
                      className={`mt-0.5 size-3.5 shrink-0 ${
                        b.severity === "high" ? "text-pill-pro-fg" : "text-pill-warn-fg"
                      }`}
                      strokeWidth={1.5}
                      aria-hidden
                    />
                    <div className="min-w-0">
                      <div className="font-medium text-ink">{b.title}</div>
                      <div className="text-ink-2">{b.impact}</div>
                    </div>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <Pill
                      className={
                        b.severity === "high"
                          ? "bg-pill-pro-bg px-2 py-0.5 text-[11px] text-pill-pro-fg"
                          : "bg-pill-warn-bg px-2 py-0.5 text-[11px] text-pill-warn-fg"
                      }
                    >
                      {b.severity}
                    </Pill>
                    {onTriggerFixAction && (
                      <Button variant="outline" size="xs" onClick={() => onTriggerFixAction(b.fix_action)}>
                        Resolve
                      </Button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          ) : (
            <div className="flex items-center gap-2 rounded-sm border border-hairline bg-pill-active-bg/30 px-3 py-2 text-xs text-pill-active-fg">
              <Check className="size-3.5" strokeWidth={2} aria-hidden />
              <span>Zero monetization blockers: retention curve, ProcessReceipt, HUD, and approved tiers aligned.</span>
            </div>
          )}
        </div>
      )}
    </section>
  );
}
