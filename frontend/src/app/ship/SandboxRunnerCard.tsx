import { useMemo, useState } from "react";
import { toast } from "sonner";
import {
  CircleCheck,
  CircleX,
  Code2,
  Cpu,
  Database,
  LayoutGrid,
  Play,
  Plus,
  Receipt,
  Terminal,
  Trash2,
  UserCheck,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import {
  runSandbox,
  type CoreLoopClaim,
  type SandboxJourneyOp,
  type SandboxJourneyStep,
  type SandboxPlayerConfig,
  type SandboxReceiptEntry,
  type SandboxRunResponse,
} from "./api";

type InspectorTab = "console" | "datastores" | "leaderstats" | "receipts" | "playergui";

type PresetId = "boot_join" | "receipt_idempotency" | "save_rejoin";

const DEFAULT_PLAYERS: SandboxPlayerConfig[] = [
  { user_id: 1001, name: "TestPlayer", owns_passes: [] },
];

const PRESETS: Record<
  PresetId,
  { label: string; description: string; steps: SandboxJourneyStep[] }
> = {
  boot_join: {
    label: "Boot + Join",
    description: "Boots Server and Client scripts, spawns player 1001, and checks runtime startup.",
    steps: [
      { op: "wait", ms: 50 },
      { op: "fire_server", remote: "ReplicatedStorage/Net/Ping", player_id: 1001, args: ["hello"] },
      { op: "assert_luau", name: "Player booted cleanly", luau: "return true", expect: { truthy: true } },
    ],
  },
  receipt_idempotency: {
    label: "Purchase Receipt Idempotency",
    description: "Fires ProcessReceipt twice with the same PurchaseId to verify single-grant idempotency.",
    steps: [
      {
        op: "purchase_receipt",
        player_id: 1001,
        product_id: 3736007337,
        purchase_id: "rcpt_idempotency_1",
        replay_duplicate: true,
      },
      {
        op: "assert_luau",
        name: "Single receipt recorded",
        luau: "return #receipts_processed == 1",
        expect: { equals: true },
      },
    ],
  },
  save_rejoin: {
    label: "Save/Rejoin Persistence",
    description: "Mutates player progression, simulates PlayerRemoving + BindToClose, then rejoins.",
    steps: [
      { op: "fire_server", remote: "ReplicatedStorage/Net/EarnCoins", player_id: 1001, args: [50] },
      { op: "rejoin", player_id: 1001 },
      {
        op: "assert_luau",
        name: "DataStore state restored",
        luau: "return leaderstats ~= nil",
        expect: { truthy: true },
      },
    ],
  },
};

const BUILDER_OPS: { op: SandboxJourneyOp; label: string }[] = [
  { op: "fire_server", label: "fire_server" },
  { op: "purchase_receipt", label: "purchase_receipt" },
  { op: "rejoin", label: "rejoin" },
  { op: "click_gui", label: "click_gui" },
  { op: "assert_luau", label: "assert_luau" },
  { op: "wait", label: "wait" },
  { op: "invoke_server", label: "invoke_server" },
];

function makeBlankStep(op: SandboxJourneyOp, idx: number): SandboxJourneyStep {
  switch (op) {
    case "fire_server":
      return { op: "fire_server", remote: "ReplicatedStorage/Net/Roll", player_id: 1001, args: [] };
    case "invoke_server":
      return { op: "invoke_server", remote: "ReplicatedStorage/Net/GetProfile", player_id: 1001, args: [] };
    case "purchase_receipt":
      return {
        op: "purchase_receipt",
        player_id: 1001,
        product_id: 3736007337,
        purchase_id: `rcpt_${idx + 1}`,
        replay_duplicate: true,
      };
    case "rejoin":
      return { op: "rejoin", player_id: 1001 };
    case "click_gui":
      return { op: "click_gui", player_id: 1001, gui_path: "MainHud.HudRoot.RollButton" };
    case "assert_luau":
      return {
        op: "assert_luau",
        name: `Assertion ${idx + 1}`,
        luau: "return true",
        expect: { truthy: true },
      };
    case "wait":
      return { op: "wait", ms: 100 };
  }
}

function extractFileLineRef(text: string): { file: string; line: number } | null {
  const bracketed = /\[string\s+"([^"]+)"\]:(\d+)/.exec(text);
  if (bracketed) {
    return { file: bracketed[1], line: Number(bracketed[2]) };
  }
  const fileMatch = /([\w./-]+\.luau?):(\d+)/.exec(text);
  if (fileMatch) {
    return { file: fileMatch[1], line: Number(fileMatch[2]) };
  }
  const dmMatch = /((?:ServerScriptService|StarterPlayer|ReplicatedStorage|StarterGui|Workspace)[\w.]+):(\d+)/.exec(text);
  if (dmMatch) {
    return { file: dmMatch[1], line: Number(dmMatch[2]) };
  }
  return null;
}

function formatDuration(ms: number): string {
  if (!Number.isFinite(ms) || ms < 0) return "0ms";
  if (ms < 1000) return `${Math.round(ms)}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function formatCoreLoop(v: CoreLoopClaim | string | undefined): string {
  if (typeof v === "string") return v;
  if (v && typeof v.status === "string" && v.status.trim()) return v.status;
  return "not tested";
}

function formatReceipt(r: SandboxReceiptEntry): string {
  if (typeof r === "string") return r;
  const pid = r.purchase_id ?? "receipt";
  const prod = r.product_id !== undefined ? `#${r.product_id}` : "#?";
  const dec = r.decision ?? "processed";
  const dup = r.duplicate ? (r.idempotent ? " (dup idempotent)" : " (dup)") : "";
  return `${pid} · ${prod} · ${dec}${dup}`;
}

function safeStringify(val: unknown): string {
  try {
    return JSON.stringify(val, null, 2) ?? "null";
  } catch {
    return String(val);
  }
}

function GuiTreeNode({ node, depth = 0 }: { node: unknown; depth?: number }) {
  if (typeof node === "string" || typeof node === "number" || typeof node === "boolean") {
    return (
      <div className="font-mono text-xs text-ink" style={{ paddingLeft: `${depth * 14}px` }}>
        {String(node)}
      </div>
    );
  }
  if (!node || typeof node !== "object") return null;
  const obj = node as Record<string, unknown>;
  const name = typeof obj.name === "string"
    ? obj.name
    : typeof obj.Name === "string"
      ? obj.Name
      : typeof obj.player_name === "string"
        ? obj.player_name
        : "Node";
  const cls = typeof obj.class_name === "string"
    ? obj.class_name
    : typeof obj.className === "string"
      ? obj.className
      : typeof obj.ClassName === "string"
        ? obj.ClassName
        : obj.player_id !== undefined
          ? "PlayerGui"
          : "Instance";
  const children = Array.isArray(obj.children) ? obj.children : [];
  return (
    <div className="flex flex-col gap-1">
      <div className="flex flex-wrap items-center gap-1.5 font-mono text-xs" style={{ paddingLeft: `${depth * 14}px` }}>
        <span className="font-medium text-ink">{name}</span>
        <span className="rounded-sm bg-board px-1.5 py-0.5 text-[11px] text-ink-2">{cls}</span>
        {typeof obj.text === "string" && obj.text && (
          <span className="truncate text-ink-3">"{obj.text}"</span>
        )}
      </div>
      {children.map((child, idx) => (
        <GuiTreeNode key={`${name}-${idx}`} node={child} depth={depth + 1} />
      ))}
    </div>
  );
}

export function SandboxRunnerCard({
  projectId,
  onJumpToIssue,
}: {
  projectId?: string | null;
  onJumpToIssue?: (scriptPath: string, line?: number) => void;
}) {
  const [activePreset, setActivePreset] = useState<PresetId>("boot_join");
  const [steps, setSteps] = useState<SandboxJourneyStep[]>(() =>
    PRESETS.boot_join.steps.map((s) => ({ ...s })),
  );
  const [builderOpen, setBuilderOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<SandboxRunResponse | null>(null);
  const [error, setError] = useState("");
  const [inspectorTab, setInspectorTab] = useState<InspectorTab>("console");
  const [argsDrafts, setArgsDrafts] = useState<Record<number, string>>({});
  const [argsErrors, setArgsErrors] = useState<Record<number, string>>({});

  const validateJourney = (journeyToRun: SandboxJourneyStep[]): string | null => {
    if (journeyToRun.length === 0) return "Add at least one journey step or select a preset.";
    const badArgIdx = Object.keys(argsErrors).find((k) => argsErrors[Number(k)]);
    if (badArgIdx !== undefined) {
      return `Step #${Number(badArgIdx) + 1}: ${argsErrors[Number(badArgIdx)]}`;
    }
    for (let i = 0; i < journeyToRun.length; i++) {
      const st = journeyToRun[i];
      if ((st.op === "fire_server" || st.op === "invoke_server") && !(st.remote || "").trim()) {
        return `Step #${i + 1} (${st.op}): remote path is required.`;
      }
      if (st.op === "click_gui" && !(st.gui_path || "").trim()) {
        return `Step #${i + 1} (click_gui): GUI path is required.`;
      }
      if (st.op === "assert_luau" && !(st.luau || "").trim()) {
        return `Step #${i + 1} (assert_luau): Luau expression is required.`;
      }
    }
    return null;
  };

  const executeRun = async (journeyToRun: SandboxJourneyStep[], presetLabel?: string, skipValidation = false) => {
    if (busy) return;
    if (!skipValidation) {
      const validationError = validateJourney(journeyToRun);
      if (validationError) {
        setError(validationError);
        setBuilderOpen(true);
        toast.error(validationError);
        return;
      }
    }
    setBusy(true);
    setError("");
    try {
      const res = await runSandbox(
        {
          timeout_secs: 15,
          players: DEFAULT_PLAYERS,
          journey: journeyToRun,
        },
        projectId,
      );
      const luneMissing = res.engine === "lune_missing" || res.diagnostics?.lune_found === false;
      if (!res.ok && !luneMissing && (res.httpStatus >= 400 || !res.engine)) {
        const msg = res.error || "Could not run the headless sandbox.";
        setError(msg);
        toast.error(msg);
        return;
      }
      const payload: SandboxRunResponse = {
        ok: Boolean(res.ok),
        engine: luneMissing ? "lune_missing" : (res.engine ?? "browser_sim"),
        passed: Boolean(res.passed),
        duration_ms: typeof res.duration_ms === "number" && Number.isFinite(res.duration_ms) ? res.duration_ms : 0,
        steps: Array.isArray(res.steps) ? res.steps : [],
        assertions: Array.isArray(res.assertions) ? res.assertions : [],
        console: {
          lines: Array.isArray(res.console?.lines) ? res.console.lines : [],
          errors: Array.isArray(res.console?.errors) ? res.console.errors : [],
          warnings: Array.isArray(res.console?.warnings) ? res.console.warnings : [],
        },
        state_snapshot: {
          leaderstats: res.state_snapshot?.leaderstats ?? {},
          datastores: res.state_snapshot?.datastores ?? {},
          receipts_processed: Array.isArray(res.state_snapshot?.receipts_processed) ? res.state_snapshot.receipts_processed : [],
          remotes_fired: typeof res.state_snapshot?.remotes_fired === "number" ? res.state_snapshot.remotes_fired : 0,
          player_gui_tree: Array.isArray(res.state_snapshot?.player_gui_tree) ? res.state_snapshot.player_gui_tree : [],
        },
        core_loop: res.core_loop ?? { move: "not tested", act: "not tested", hud: "not tested" },
        diagnostic: res.diagnostic ?? (luneMissing ? res.error : undefined),
        diagnostics: res.diagnostics,
      };
      setResult(payload);
      if (payload.console.errors.length > 0 || payload.steps.some((s) => !s.passed)) {
        setInspectorTab("console");
      }
      const label = presetLabel || "Sandbox journey";
      if (luneMissing) {
        toast.error(payload.diagnostic || "Lune runtime not detected on PATH.");
      } else if (payload.passed) {
        toast.success(`${label} passed in ${formatDuration(payload.duration_ms)}`);
      } else {
        toast.error(`${label} finished with failures.`);
      }
    } catch {
      const msg = "Sandbox request failed unexpectedly. Check your journey steps and retry.";
      setError(msg);
      toast.error(msg);
    } finally {
      setBusy(false);
    }
  };

  const applyPreset = (id: PresetId) => {
    setActivePreset(id);
    const cloned = PRESETS[id].steps.map((s) => ({ ...s }));
    setSteps(cloned);
    setArgsDrafts({});
    setArgsErrors({});
    setError("");
    void executeRun(cloned, PRESETS[id].label, true);
  };

  const updateStep = (idx: number, patch: Partial<SandboxJourneyStep>) => {
    setSteps((prev) => prev.map((s, i) => (i === idx ? { ...s, ...patch } : s)));
  };

  const replaceStepOp = (idx: number, nextOp: SandboxJourneyOp) => {
    setSteps((prev) => prev.map((s, i) => (i === idx ? makeBlankStep(nextOp, idx) : s)));
    setArgsDrafts((prev) => {
      const next = { ...prev };
      delete next[idx];
      return next;
    });
    setArgsErrors((prev) => {
      const next = { ...prev };
      delete next[idx];
      return next;
    });
  };

  const removeStep = (idx: number) => {
    setSteps((prev) => prev.filter((_, i) => i !== idx));
    const reindex = <T,>(rec: Record<number, T>): Record<number, T> => {
      const next: Record<number, T> = {};
      for (const [k, v] of Object.entries(rec)) {
        const ki = Number(k);
        if (ki < idx) next[ki] = v;
        else if (ki > idx) next[ki - 1] = v;
      }
      return next;
    };
    setArgsDrafts((prev) => reindex(prev));
    setArgsErrors((prev) => reindex(prev));
  };

  const addStep = (op: SandboxJourneyOp) => {
    setSteps((prev) => [...prev, makeBlankStep(op, prev.length)]);
    setBuilderOpen(true);
  };

  const consoleEntries = useMemo(() => {
    if (!result) return [];
    const items: { kind: "error" | "warning" | "line"; text: string }[] = [];
    for (const e of result.console.errors) items.push({ kind: "error", text: e });
    for (const w of result.console.warnings) items.push({ kind: "warning", text: w });
    for (const l of result.console.lines) items.push({ kind: "line", text: l });
    for (const st of result.steps) {
      if (st.error) items.push({ kind: "error", text: `Step #${st.index + 1} (${st.op}): ${st.error}` });
    }
    return items;
  }, [result]);

  const engineLabel = (result?.engine === "lune_missing" || result?.diagnostics?.lune_found === false)
    ? "lune_missing"
    : result?.engine === "lune_roblox_shim"
      ? "lune_roblox_shim"
      : result?.engine === "browser_sim"
        ? "browser_sim"
        : null;

  return (
    <section
      data-sandbox-runner
      aria-label="Headless Luau Runtime Sandbox"
      className="card-stock flex flex-col gap-3 p-4"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <Cpu className="size-4 text-blue" strokeWidth={1.5} aria-hidden />
            <h3 className="font-heading text-ui font-semibold text-ink">
              Headless Luau Runtime Sandbox
            </h3>
            {result && engineLabel && (
              <Pill
                data-sandbox-engine={engineLabel}
                className={
                  engineLabel === "lune_missing"
                    ? "bg-pill-warn-bg px-2 py-0.5 font-mono text-xs text-pill-warn-fg"
                    : "bg-tag-sky-bg px-2 py-0.5 font-mono text-xs text-tag-sky-fg"
                }
              >
                {engineLabel}
              </Pill>
            )}
            {result && (
              <Pill
                data-sandbox-duration
                className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2"
              >
                {formatDuration(result.duration_ms)}
              </Pill>
            )}
          </div>
          <p className="mt-0.5 text-sm text-ink-2">
            Runs Server and Client Luau scripts, RemoteEvents, ProcessReceipt idempotency, and DataStore rejoin checks locally without launching Studio.
          </p>
        </div>
        <Button
          variant="default"
          size="lg"
          loading={busy}
          disabled={busy || steps.length === 0}
          onClick={() => void executeRun(steps)}
          data-sandbox-run
          className="max-md:min-h-11"
        >
          <Play className="size-4" strokeWidth={1.5} aria-hidden />
          {busy ? "Running…" : "Run Sandbox"}
        </Button>
      </div>

      <div className="flex flex-wrap items-center gap-2" role="group" aria-label="Sandbox preset journeys">
        {(Object.keys(PRESETS) as PresetId[]).map((presetId) => {
          const preset = PRESETS[presetId];
          const isCurrent = activePreset === presetId;
          return (
            <Button
              key={presetId}
              variant={isCurrent ? "secondary" : "outline"}
              size="sm"
              disabled={busy}
              aria-pressed={isCurrent}
              data-sandbox-preset={presetId}
              onClick={() => applyPreset(presetId)}
              className="max-md:min-h-11"
            >
              <Play className="size-3.5" strokeWidth={1.5} aria-hidden />
              {preset.label}
            </Button>
          );
        })}
      </div>

      <Expandable expanded={builderOpen} onToggle={() => setBuilderOpen((v) => !v)} expandDirection="vertical" expandBehavior="push">
        <div className="rounded-card border border-hairline bg-board/50 px-3 py-2">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <ExpandableTrigger
              aria-label="Toggle Journey Step Builder"
              data-sandbox-builder-toggle
              className="flex min-h-11 cursor-pointer items-center gap-2 text-left text-sm font-medium text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus"
            >
              <span>Journey Steps</span>
              <Pill className="bg-surface px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
                {steps.length}
              </Pill>
            </ExpandableTrigger>
            <div className="flex flex-wrap items-center gap-1.5">
              {(["fire_server", "purchase_receipt", "rejoin", "click_gui", "assert_luau"] as const).map((op) => (
                <Button
                  key={op}
                  variant="outline"
                  size="xs"
                  data-sandbox-add-step={op}
                  onClick={() => addStep(op)}
                  className="max-md:min-h-11"
                >
                  <Plus className="size-3" strokeWidth={1.5} aria-hidden />
                  {op}
                </Button>
              ))}
            </div>
          </div>

          {builderOpen && (
            <div className="mt-2 flex flex-col gap-2 border-t border-hairline pt-2" data-sandbox-builder>
              {steps.length === 0 ? (
                <p className="text-xs text-ink-2">No steps configured. Add a step above or pick a preset.</p>
              ) : (
                <ul className="flex flex-col gap-2" aria-label="Journey steps">
                  {steps.map((step, idx) => (
                    <li
                      key={`${step.op}-${idx}`}
                      data-sandbox-step-editor={idx}
                      className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
                    >
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-mono text-ink-2">#{idx + 1}</span>
                          <select
                            aria-label={`Step ${idx + 1} operation`}
                            value={step.op}
                            onChange={(e) => replaceStepOp(idx, e.target.value as SandboxJourneyOp)}
                            className="min-h-8 rounded-sm border border-hairline bg-board px-2 font-mono text-xs text-ink outline-none focus-visible:ring-2 focus-visible:ring-focus"
                          >
                            {BUILDER_OPS.map((o) => (
                              <option key={o.op} value={o.op}>
                                {o.label}
                              </option>
                            ))}
                          </select>
                        </div>
                        <Button
                          variant="ghost"
                          size="xs"
                          aria-label={`Remove step ${idx + 1}`}
                          onClick={() => removeStep(idx)}
                          className="max-md:min-h-11"
                        >
                          <Trash2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                        </Button>
                      </div>

                      <div className="grid gap-2 sm:grid-cols-2">
                        {(step.op === "fire_server" || step.op === "invoke_server") && (
                          <>
                            <Input
                              aria-label={`Step ${idx + 1} remote path`}
                              value={step.remote ?? ""}
                              onChange={(e) => updateStep(idx, { remote: e.target.value })}
                              placeholder="ReplicatedStorage/Net/Roll"
                              className="h-8 font-mono text-xs"
                            />
                            <Input
                              aria-label={`Step ${idx + 1} args JSON`}
                              aria-invalid={Boolean(argsErrors[idx])}
                              data-sandbox-step-args={idx}
                              value={argsDrafts[idx] ?? JSON.stringify(step.args ?? [])}
                              onChange={(e) => {
                                const raw = e.target.value;
                                setArgsDrafts((prev) => ({ ...prev, [idx]: raw }));
                                try {
                                  const parsed = JSON.parse(raw);
                                  if (Array.isArray(parsed)) {
                                    updateStep(idx, { args: parsed });
                                    setArgsErrors((prev) => {
                                      const next = { ...prev };
                                      delete next[idx];
                                      return next;
                                    });
                                    if (error) setError("");
                                  } else {
                                    setArgsErrors((prev) => ({
                                      ...prev,
                                      [idx]: 'Args must be a JSON array, such as ["arg1"].',
                                    }));
                                  }
                                } catch {
                                  setArgsErrors((prev) => ({
                                    ...prev,
                                    [idx]: 'Invalid JSON in step args. Expected a JSON array like ["arg1"].',
                                  }));
                                }
                              }}
                              placeholder='["arg1"]'
                              className="h-8 font-mono text-xs"
                            />
                            {argsErrors[idx] && (
                              <p
                                role="alert"
                                data-sandbox-step-error={idx}
                                className="text-xs text-pill-pro-fg sm:col-span-2"
                              >
                                {argsErrors[idx]}
                              </p>
                            )}
                          </>
                        )}

                        {step.op === "purchase_receipt" && (
                          <>
                            <Input
                              aria-label={`Step ${idx + 1} product ID`}
                              inputMode="numeric"
                              value={String(step.product_id ?? "")}
                              onChange={(e) =>
                                updateStep(idx, {
                                  product_id: Number(e.target.value.replace(/\D/g, "")) || 0,
                                })
                              }
                              placeholder="3736007337"
                              className="h-8 font-mono text-xs"
                            />
                            <Input
                              aria-label={`Step ${idx + 1} purchase ID`}
                              value={step.purchase_id ?? ""}
                              onChange={(e) => updateStep(idx, { purchase_id: e.target.value })}
                              placeholder="rcpt_1"
                              className="h-8 font-mono text-xs"
                            />
                            <label className="flex items-center gap-2 text-xs text-ink-2 sm:col-span-2">
                              <input
                                type="checkbox"
                                checked={Boolean(step.replay_duplicate)}
                                onChange={(e) => updateStep(idx, { replay_duplicate: e.target.checked })}
                              />
                              <span>Replay duplicate PurchaseId to verify idempotency</span>
                            </label>
                          </>
                        )}

                        {step.op === "rejoin" && (
                          <Input
                            aria-label={`Step ${idx + 1} player ID`}
                            inputMode="numeric"
                            value={String(step.player_id ?? 1001)}
                            onChange={(e) =>
                              updateStep(idx, {
                                player_id: Number(e.target.value.replace(/\D/g, "")) || 1001,
                              })
                            }
                            placeholder="1001"
                            className="h-8 font-mono text-xs"
                          />
                        )}

                        {step.op === "click_gui" && (
                          <Input
                            aria-label={`Step ${idx + 1} GUI path`}
                            value={step.gui_path ?? ""}
                            onChange={(e) => updateStep(idx, { gui_path: e.target.value })}
                            placeholder="MainHud.HudRoot.RollButton"
                            className="h-8 font-mono text-xs sm:col-span-2"
                          />
                        )}

                        {step.op === "wait" && (
                          <Input
                            aria-label={`Step ${idx + 1} wait milliseconds`}
                            inputMode="numeric"
                            value={String(step.ms ?? 100)}
                            onChange={(e) =>
                              updateStep(idx, { ms: Number(e.target.value.replace(/\D/g, "")) || 0 })
                            }
                            placeholder="100"
                            className="h-8 font-mono text-xs"
                          />
                        )}

                        {step.op === "assert_luau" && (
                          <>
                            <Input
                              aria-label={`Step ${idx + 1} assertion name`}
                              value={step.name ?? ""}
                              onChange={(e) => updateStep(idx, { name: e.target.value })}
                              placeholder="Assertion label"
                              className="h-8 text-xs"
                            />
                            <Input
                              aria-label={`Step ${idx + 1} Luau expression`}
                              value={step.luau ?? ""}
                              onChange={(e) => updateStep(idx, { luau: e.target.value })}
                              placeholder="return #receipts_processed == 1"
                              className="h-8 font-mono text-xs"
                            />
                          </>
                        )}
                      </div>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
        </div>
      </Expandable>

      {error && (
        <div
          role="alert"
          data-sandbox-error
          className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg"
        >
          <span className="min-w-0 flex-1 break-words">{error}</span>
          <div className="flex shrink-0 items-center gap-1.5">
            <Button
              variant="outline"
              size="xs"
              disabled={busy}
              onClick={() => void executeRun(steps)}
              className="bg-surface max-md:min-h-11"
            >
              Retry
            </Button>
            <Button
              variant="ghost"
              size="xs"
              disabled={busy}
              onClick={() => applyPreset("boot_join")}
              className="max-md:min-h-11"
            >
              Reset preset
            </Button>
          </div>
        </div>
      )}

      {result && (
        <div className="flex flex-col gap-3" data-sandbox-result>
          <div className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/50 px-3 py-2">
            <div className="flex flex-wrap items-center gap-2">
              <Pill
                data-sandbox-verdict={result.passed ? "passed" : "failed"}
                className={`gap-1.5 px-2.5 py-0.5 text-xs ${
                  result.passed
                    ? "bg-pill-active-bg text-pill-active-fg"
                    : "bg-pill-pro-bg text-pill-pro-fg"
                }`}
              >
                {result.passed ? (
                  <CircleCheck className="size-3.5" strokeWidth={1.5} aria-hidden />
                ) : (
                  <CircleX className="size-3.5" strokeWidth={1.5} aria-hidden />
                )}
                {result.passed ? "Sandbox Passed" : "Sandbox Failed"}
              </Pill>
              <span className="font-mono text-xs tabular-nums text-ink-2">
                {result.steps.filter((s) => s.passed).length}/{result.steps.length} steps
              </span>
              {result.assertions.length > 0 && (
                <span className="font-mono text-xs tabular-nums text-ink-2">
                  {result.assertions.filter((a) => a.passed).length}/{result.assertions.length} assertions
                </span>
              )}
            </div>
            <div className="flex flex-wrap items-center gap-1.5 text-xs">
              <Pill className="bg-surface px-2 py-0.5 text-ink-2">Move: {formatCoreLoop(result.core_loop.move)}</Pill>
              <Pill className="bg-surface px-2 py-0.5 text-ink-2">Act: {formatCoreLoop(result.core_loop.act)}</Pill>
              <Pill className="bg-surface px-2 py-0.5 text-ink-2">HUD: {formatCoreLoop(result.core_loop.hud)}</Pill>
            </div>
          </div>

          {result.diagnostic && (
            <p
              data-sandbox-diagnostic
              className="rounded-sm border border-hairline bg-pill-warn-bg/40 px-3 py-2 text-xs text-ink"
            >
              {result.diagnostic}
            </p>
          )}

          {result.steps.length > 0 && (
            <ul className="grid gap-1.5 sm:grid-cols-2" data-sandbox-steps aria-label="Sandbox step results">
              {result.steps.map((st) => (
                <li
                  key={`${st.index}-${st.op}`}
                  data-sandbox-step-result={st.index}
                  className="flex items-start justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-2.5 py-2 text-xs"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span className="font-mono text-ink-2">#{st.index + 1}</span>
                      <span className="font-mono font-medium text-ink">{st.op}</span>
                      <span className="font-mono tabular-nums text-ink-3">{formatDuration(st.duration_ms)}</span>
                    </div>
                    {st.error ? (
                      <p className="mt-0.5 break-words font-mono text-pill-pro-fg">{st.error}</p>
                    ) : st.result !== undefined ? (
                      <p className="mt-0.5 truncate font-mono text-ink-2">{JSON.stringify(st.result)}</p>
                    ) : null}
                  </div>
                  <Pill
                    className={`shrink-0 px-2 py-0.5 text-xs ${
                      st.passed
                        ? "bg-pill-active-bg text-pill-active-fg"
                        : "bg-pill-pro-bg text-pill-pro-fg"
                    }`}
                  >
                    {st.passed ? "Pass" : "Fail"}
                  </Pill>
                </li>
              ))}
            </ul>
          )}

          {result.assertions.length > 0 && (
            <ul className="flex flex-col gap-1.5" data-sandbox-assertions aria-label="Sandbox assertions">
              {result.assertions.map((a, idx) => (
                <li
                  key={`${a.name}-${idx}`}
                  className="flex flex-wrap items-center justify-between gap-2 rounded-sm border border-hairline bg-board/40 px-2.5 py-1.5 text-xs"
                >
                  <div className="min-w-0 flex-1">
                    <span className="font-medium text-ink">{a.name}</span>
                    {a.error && <span className="ml-2 font-mono text-pill-pro-fg">{a.error}</span>}
                  </div>
                  <Pill
                    className={`shrink-0 px-2 py-0.5 text-xs ${
                      a.passed
                        ? "bg-pill-active-bg text-pill-active-fg"
                        : "bg-pill-pro-bg text-pill-pro-fg"
                    }`}
                  >
                    {a.passed ? "Pass" : "Fail"}
                  </Pill>
                </li>
              ))}
            </ul>
          )}

          <div className="flex flex-col gap-2" data-sandbox-inspector>
            <SegmentedControl
              aria-label="Sandbox console and state inspector"
              size="sm"
              className="max-w-full flex-wrap"
            >
              <SegmentedControlItem
                data-checked={inspectorTab === "console" ? "" : undefined}
                aria-pressed={inspectorTab === "console"}
                data-sandbox-tab="console"
                onClick={() => setInspectorTab("console")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <Terminal className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Console ({consoleEntries.length})</span>
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={inspectorTab === "datastores" ? "" : undefined}
                aria-pressed={inspectorTab === "datastores"}
                data-sandbox-tab="datastores"
                onClick={() => setInspectorTab("datastores")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <Database className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>DataStores</span>
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={inspectorTab === "leaderstats" ? "" : undefined}
                aria-pressed={inspectorTab === "leaderstats"}
                data-sandbox-tab="leaderstats"
                onClick={() => setInspectorTab("leaderstats")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <UserCheck className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>leaderstats</span>
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={inspectorTab === "receipts" ? "" : undefined}
                aria-pressed={inspectorTab === "receipts"}
                data-sandbox-tab="receipts"
                onClick={() => setInspectorTab("receipts")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <Receipt className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>Receipts ({result.state_snapshot.receipts_processed.length})</span>
              </SegmentedControlItem>
              <SegmentedControlItem
                data-checked={inspectorTab === "playergui" ? "" : undefined}
                aria-pressed={inspectorTab === "playergui"}
                data-sandbox-tab="playergui"
                onClick={() => setInspectorTab("playergui")}
                className="gap-1 max-md:!h-11 max-md:px-2.5"
              >
                <LayoutGrid className="size-3.5" strokeWidth={1.5} aria-hidden />
                <span>PlayerGui ({result.state_snapshot.player_gui_tree.length})</span>
              </SegmentedControlItem>
            </SegmentedControl>

            <div
              data-sandbox-inspector-body={inspectorTab}
              className="max-h-60 overflow-auto rounded-card border border-hairline bg-board/60 p-3 text-xs"
            >
              {inspectorTab === "console" && (
                consoleEntries.length === 0 ? (
                  <p className="text-ink-2">No console output recorded for this run.</p>
                ) : (
                  <ul className="flex flex-col gap-1.5 font-mono">
                    {consoleEntries.map((entry, i) => {
                      const ref = extractFileLineRef(entry.text);
                      return (
                        <li
                          key={i}
                          className="flex flex-wrap items-center justify-between gap-2 rounded-sm bg-surface/70 px-2 py-1"
                        >
                          <span
                            className={`min-w-0 flex-1 break-words ${
                              entry.kind === "error"
                                ? "text-pill-pro-fg"
                                : entry.kind === "warning"
                                  ? "text-pill-warn-fg"
                                  : "text-ink-2"
                            }`}
                          >
                            {entry.text}
                          </span>
                          {ref && onJumpToIssue && (
                            <Button
                              variant="outline"
                              size="xs"
                              onClick={() => onJumpToIssue(ref.file, ref.line)}
                              data-sandbox-jump={`${ref.file}:${ref.line}`}
                              className="max-md:min-h-11"
                            >
                              <Code2 className="size-3" strokeWidth={1.5} aria-hidden />
                              {ref.file}:{ref.line}
                            </Button>
                          )}
                        </li>
                      );
                    })}
                  </ul>
                )
              )}

              {inspectorTab === "datastores" && (
                <pre className="whitespace-pre-wrap break-words font-mono text-ink-2">
                  {safeStringify(result.state_snapshot.datastores)}
                </pre>
              )}

              {inspectorTab === "leaderstats" && (
                <pre className="whitespace-pre-wrap break-words font-mono text-ink-2">
                  {safeStringify(result.state_snapshot.leaderstats)}
                </pre>
              )}

              {inspectorTab === "receipts" && (
                <div className="flex flex-col gap-2">
                  <div className="flex flex-wrap items-center gap-3 text-ink-2">
                    <span>
                      Remotes fired:{" "}
                      <span className="font-mono font-medium tabular-nums text-ink">
                        {result.state_snapshot.remotes_fired}
                      </span>
                    </span>
                    <span>
                      Receipts processed:{" "}
                      <span className="font-mono font-medium tabular-nums text-ink">
                        {result.state_snapshot.receipts_processed.length}
                      </span>
                    </span>
                  </div>
                  {result.state_snapshot.receipts_processed.length === 0 ? (
                    <p className="text-ink-2">No receipts processed during this run.</p>
                  ) : (
                    <ul className="flex flex-wrap gap-1.5">
                      {result.state_snapshot.receipts_processed.map((rid, idx) => (
                        <li key={typeof rid === "string" ? `${rid}-${idx}` : `${rid.purchase_id ?? "rcpt"}-${idx}`}>
                          <Pill className="bg-surface px-2 py-0.5 font-mono text-xs text-ink">
                            {formatReceipt(rid)}
                          </Pill>
                        </li>
                      ))}
                    </ul>
                  )}
                </div>
              )}

              {inspectorTab === "playergui" && (
                result.state_snapshot.player_gui_tree.length === 0 ? (
                  <p className="text-ink-2">No PlayerGui nodes captured during this run.</p>
                ) : (
                  <div className="flex flex-col gap-2">
                    {result.state_snapshot.player_gui_tree.map((node, i) => (
                      <GuiTreeNode key={i} node={node} />
                    ))}
                  </div>
                )
              )}
            </div>
          </div>
        </div>
      )}
    </section>
  );
}
