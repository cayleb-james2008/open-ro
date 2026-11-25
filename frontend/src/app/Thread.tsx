import {
  AlertTriangle,
  Box,
  Check,
  ChevronDown,
  FileCode,
  FileDiff,
  GitPullRequest,
  RefreshCw,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { BorderTrail, TextShimmer } from "@/components/ui";
import { Button } from "@/components/ui/button";
import { AdaptiveAgentCards } from "./AdaptiveAgentCards";
import type { CreatorHubSetupResult } from "./ship/api";
import { Spinner } from "@/components/ui/spinner";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillIndicator, PillStatus } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { useReducedMotion } from "@/motion";
import {
  BUILD_PHASES,
  FAILURE,
  agentText,
  classify,
  plain,
  stepInfo,
  type BuildPhaseId,
  type BuildStreamPhase,
  type Idea,
  type PendingProposal,
  type Row,
  type ScriptFileDiff,
} from "./data";
import { BlueprintCard } from "./Blueprints";

const STEP_ICONS = {
  doc: FileCode,
  box: Box,
  trash: Trash2,
  check: Check,
} as const;

const PHASE_ORDER: BuildPhaseId[] = ["planning", "compiling", "checking", "playtest", "repair"];

export function WorkingBanner({ label, phase }: { label: string; phase?: BuildStreamPhase | null }) {
  const reduce = useReducedMotion();
  const activeIdx = phase ? Math.max(0, PHASE_ORDER.indexOf(phase.phase)) : 0;
  const pct = phase?.progress ?? Math.min(96, 20 + activeIdx * 24);
  return (
    <div
      role="status"
      aria-live="polite"
      className="relative flex flex-col gap-2.5 rounded-card border border-hairline bg-surface px-4 py-3 text-sm text-ink"
    >
      <BorderTrail active={!reduce} />
      <div className="flex items-center gap-3">
        <Spinner className="size-4 text-blue" />
        <div className="min-w-0 flex-1">
          <TextShimmer active={!reduce} className="block font-medium [overflow-wrap:anywhere]">{label}</TextShimmer>
          {phase?.detail && (
            <span className="block text-xs text-ink-2 [overflow-wrap:anywhere]">{phase.detail}</span>
          )}
        </div>
        <Pill className="bg-board text-xs text-ink-2">
          <PillStatus className="border-transparent">
            <PillIndicator variant="info" pulse={!reduce} />
            <span>{phase?.label || "Running"}</span>
          </PillStatus>
        </Pill>
      </div>

      {phase && (
        <div className="flex flex-wrap items-center justify-between gap-2 border-t border-hairline pt-2">
          <div className="flex flex-wrap items-center gap-1.5">
            {BUILD_PHASES.map((step, idx) => {
              const done = idx < activeIdx;
              const current = idx === activeIdx || (phase.phase === "repair" && step.id === "playtest");
              return (
                <span
                  key={step.id}
                  className={`inline-flex h-6 items-center gap-1 rounded-chip px-2 text-xs font-medium ${
                    done
                      ? "bg-pill-active-bg text-pill-active-fg"
                      : current
                        ? "bg-blue-soft text-ink"
                        : "bg-track text-ink-3"
                  }`}
                >
                  {done && <Check className="size-3" strokeWidth={2} aria-hidden />}
                  <span>{step.label}</span>
                </span>
              );
            })}
            {phase.phase === "repair" && (
              <Pill className="bg-pill-warn-bg px-2 py-0.5 text-xs text-pill-warn-fg">Auto-Repair</Pill>
            )}
          </div>
          <span className="font-mono text-xs tabular-nums text-ink-2">{Math.round(pct)}%</span>
        </div>
      )}
    </div>
  );
}

export function UserPrompt({ text }: { text: string; fresh?: boolean }) {
  return (
    <div className="flex justify-end">
      <div className="max-w-[min(86%,580px)] rounded-panel rounded-tr-sm border border-hairline bg-ink-button px-4 py-2.5 text-sm leading-relaxed text-ink-button-fg whitespace-pre-wrap [overflow-wrap:anywhere]">
        {text}
      </div>
    </div>
  );
}

export function AgentMessage({
  text,
  bad,
  whole,
  fresh,
  projectId,
  onPrefillPrompt,
  onQuickCreatorHubSetup,
  onOpenShip,
  onRetry,
  retryDisabled,
}: {
  text: string;
  bad?: boolean;
  whole?: boolean;
  fresh?: boolean;
  projectId?: string | null;
  onPrefillPrompt?: (prompt: string) => void;
  onQuickCreatorHubSetup?: () => Promise<CreatorHubSetupResult | void>;
  onOpenShip?: () => void;
  onRetry?: () => void;
  retryDisabled?: boolean;
}) {
  const { plain: say, raw } = agentText(text, !!bad, !!whole);
  const needsSummary = !whole && (say.length > 160 || say.includes("\n"));
  const shortSay = needsSummary
    ? (() => {
        const firstLine = say.split("\n")[0].trim();
        const sentEnd = firstLine.search(/[.!?](?:\s|$)/);
        const cut = sentEnd > 24 && sentEnd < 150 ? firstLine.slice(0, sentEnd + 1) : firstLine.slice(0, 148).trimEnd() + "...";
        return cut;
      })()
    : say;
  const detailsBody = raw || (needsSummary ? say : "");
  return (
    <div
      role={bad ? "alert" : undefined}
      className={`flex flex-col gap-2 rounded-card border p-3.5 text-sm leading-relaxed ${
        bad
          ? "border-pill-pro-fg/35 bg-pill-pro-bg text-pill-pro-fg"
          : "border-hairline bg-surface text-ink"
      }`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <p className="min-w-0 flex-1 [overflow-wrap:anywhere]">{shortSay}</p>
        {bad && onRetry && (
          <Button
            variant="outline"
            size="sm"
            disabled={retryDisabled}
            onClick={onRetry}
            className="bg-surface text-ink max-md:min-h-11"
          >
            <RefreshCw className="size-3.5" strokeWidth={1.5} aria-hidden />
            Try again
          </Button>
        )}
      </div>

      <AdaptiveAgentCards
        text={text}
        projectId={projectId}
        onPrefillPrompt={onPrefillPrompt}
        onQuickCreatorHubSetup={onQuickCreatorHubSetup}
        onOpenShip={onOpenShip}
        onRetry={onRetry}
        retryDisabled={retryDisabled}
      />

      {detailsBody && (
        <Expandable expandDirection="vertical" expandBehavior="push">
          {({ isExpanded }) => (
            <div className="border-t border-hairline/70 pt-2">
              <ExpandableTrigger
                aria-label={isExpanded ? "Hide message details" : "Show message details"}
                className="inline-flex min-h-11 items-center gap-1 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-board hover:text-ink md:min-h-7"
              >
                <span>Details</span>
                <ChevronDown
                  className={`size-3.5 duration-(--dur-swap) ease-(--ease-house) motion-safe:transition-transform ${isExpanded ? "rotate-180" : ""}`}
                  strokeWidth={1.5}
                  aria-hidden
                />
              </ExpandableTrigger>
              <ExpandableContent preset="fade">
                <pre className="mt-1.5 whitespace-pre-wrap break-all rounded-sm bg-board p-2.5 font-mono text-xs text-ink-2">
                  {detailsBody}
                </pre>
              </ExpandableContent>
            </div>
          )}
        </Expandable>
      )}
    </div>
  );
}

export function BuildTrace({
  rows,
  working = false,
  fileDiffs = [],
  onOpenScriptDiff,
}: {
  rows: Row[];
  working?: boolean;
  fileDiffs?: ScriptFileDiff[];
  onOpenScriptDiff?: (path: string) => void;
}) {
  const rawSteps = rows.filter((r): r is Extract<Row, { kind: "step" }> => r.kind === "step");
  const verdict = rows.find((r): r is Extract<Row, { kind: "verdict" }> => r.kind === "verdict");
  const consoleRows = rows.filter((r): r is Extract<Row, { kind: "console" }> => r.kind === "console");
  const rawInfos = rawSteps.map((s) => stepInfo(s.raw));
  const changeCount = rawInfos.filter((i) => i.changed).length;
  const generated = rawInfos.some((i) => i.stamp === "generated");
  const applied = rawInfos.some((i) => i.changed);
  const doneStages = [generated || applied, applied, !!verdict].filter(Boolean).length;
  const consoleText = consoleRows.map((c) => c.text).join("\n");
  const diffByPath = new Map(fileDiffs.map((d) => [d.path, d]));

  // Fold "Generated ... script" immediately followed by "Wrote ..." into a single clean Studio step row.
  const displaySteps: { raw: string; info: ReturnType<typeof stepInfo> }[] = [];
  for (let i = 0; i < rawSteps.length; i++) {
    const cur = rawInfos[i];
    const next = rawInfos[i + 1];
    if (cur.stamp === "generated" && next?.stamp === "saved") {
      displaySteps.push({
        raw: `${rawSteps[i].raw}\n${rawSteps[i + 1].raw}`,
        info: { ...next, label: next.label.replace(/^Saved script /, "Generated & saved ") },
      });
      i++;
    } else {
      displaySteps.push({ raw: rawSteps[i].raw, info: cur });
    }
  }

  return (
    <section
      aria-label="Studio build trace"
      className={`overflow-hidden rounded-card border bg-surface ${
        working ? "border-blue ring-2 ring-blue/25" : "border-hairline"
      }`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2.5 border-b border-hairline bg-board/60 px-3.5 py-2.5">
        <div className="flex min-w-0 items-center gap-2.5">
          <ProgressCircle
            value={Math.round((doneStages / 3) * 100)}
            radius={15}
            strokeWidth={3.5}
            className="size-8 flex-none"
          >
            <span className="font-mono text-xs font-medium tabular-nums text-ink">{doneStages}/3</span>
          </ProgressCircle>

          <div className="min-w-0">
            <div className="inline-flex items-center gap-1.5 whitespace-nowrap text-xs text-ink-2">
              <span className="font-mono font-medium tabular-nums text-ink">
                <SlidingNumber value={changeCount} />
              </span>
              {`${changeCount === 1 ? "change" : "changes"} in Studio`}
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1">
          <TraceStagePill label="Luau" done={generated} />
          <TraceStagePill label="Studio" done={applied} />
          <TraceStagePill label="Playtest" done={!!verdict} />
        </div>
      </div>

      {displaySteps.length > 0 && (
        <ol className="m-0 divide-y divide-hairline list-none p-0">
          {displaySteps.map(({ raw, info }, idx) => {
            const Icon = STEP_ICONS[info.icon];
            const matchedDiff = info.target ? diffByPath.get(info.target) : undefined;
            const isScriptStep = Boolean(info.target && (info.stamp === "saved" || matchedDiff));
            return (
              <li key={idx} className="px-3.5 py-2">
                <Expandable expandDirection="vertical" expandBehavior="push">
                  {({ isExpanded }) => (
                    <>
                      <div className="flex items-start gap-2.5">
                        <span className="mt-0.5 grid size-6 flex-none place-items-center rounded-sm bg-board text-icon">
                          <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
                            <span className="text-xs font-medium text-ink [overflow-wrap:anywhere]">
                              {info.label}
                            </span>
                            {info.target && isScriptStep && onOpenScriptDiff ? (
                              <Button
                                variant="outline"
                                size="xs"
                                onClick={() => onOpenScriptDiff(info.target!)}
                                aria-label={`Inspect ${info.target} in Code & Diff`}
                                className="h-6 max-w-full gap-1 bg-board/60 px-1.5 font-mono text-xs text-ink hover:bg-board max-md:min-h-11"
                              >
                                <FileCode className="size-3 shrink-0 text-icon" strokeWidth={1.5} aria-hidden />
                                <span className="truncate">{info.target.split(".").pop() || info.target}</span>
                                {matchedDiff && (
                                  <>
                                    <span className="text-pill-active-fg">+{matchedDiff.additions}</span>
                                    <span className="text-pill-pro-fg">-{matchedDiff.deletions}</span>
                                  </>
                                )}
                              </Button>
                            ) : info.target ? (
                              <code className="max-w-full rounded-sm bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2 [overflow-wrap:anywhere]">
                                {info.target}
                              </code>
                            ) : null}
                          </div>
                        </div>
                        <div className="flex flex-none items-center gap-1">
                          <span className="font-mono text-xs text-ink-2">{info.result}</span>
                          <ExpandableTrigger
                            aria-label={`${isExpanded ? "Hide" : "Show"} step output for ${info.label}`}
                            className="inline-flex min-h-11 min-w-11 items-center justify-center rounded-sm px-1.5 text-xs text-ink-2 outline-none hover:bg-board hover:text-ink md:min-h-7 md:min-w-7"
                          >
                            <ChevronDown
                              className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                              strokeWidth={1.5}
                              aria-hidden
                            />
                          </ExpandableTrigger>
                        </div>
                      </div>
                      <ExpandableContent preset="fade">
                        <pre className="mt-2 whitespace-pre-wrap break-all rounded-sm bg-board p-2.5 font-mono text-xs text-ink-2">
                          {raw}
                        </pre>
                      </ExpandableContent>
                    </>
                  )}
                </Expandable>
              </li>
            );
          })}
        </ol>
      )}

      {fileDiffs.length > 0 && onOpenScriptDiff && (
        <div className="flex flex-wrap items-center gap-1.5 border-t border-hairline bg-board/40 px-3.5 py-2">
          <span className="inline-flex items-center gap-1 text-xs font-medium text-ink-2">
            <FileDiff className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />
            Diffs:
          </span>
          {fileDiffs.slice(0, 6).map((fd) => (
            <Button
              key={fd.path}
              variant="outline"
              size="xs"
              onClick={() => onOpenScriptDiff(fd.path)}
              aria-label={`Open diff for ${fd.path}`}
              className="gap-1 bg-surface font-mono text-xs max-md:min-h-11"
            >
              <span className="max-w-[10rem] truncate">{fd.path.split(".").pop() || fd.path}</span>
              <span className="text-pill-active-fg">+{fd.additions}</span>
              <span className="text-pill-pro-fg">-{fd.deletions}</span>
            </Button>
          ))}
        </div>
      )}

      {verdict && <VerdictBanner tone={verdict.tone} raw={verdict.raw} consoleText={consoleText} />}
    </section>
  );
}

export function ProposalDiffCard({
  proposal,
  busy = false,
  onInspect,
  onApply,
  onReject,
}: {
  proposal: PendingProposal;
  busy?: boolean;
  onInspect: (proposalId: string, scriptPath?: string) => void;
  onApply: (proposalId: string) => void;
  onReject: (proposalId: string) => void;
}) {
  const totalAdded = proposal.file_diffs.reduce((s, d) => s + d.additions, 0);
  const totalRemoved = proposal.file_diffs.reduce((s, d) => s + d.deletions, 0);
  const errCount = proposal.diagnostics.filter((d) => d.severity === "error").length;
  const instancesAdded = proposal.diff?.added.filter((x) => x.kind === "instance").length ?? 0;
  const isPending = proposal.status === "pending";

  const statusTone =
    proposal.status === "applied"
      ? { label: "Applied", cls: "bg-pill-active-bg text-pill-active-fg" }
      : proposal.status === "rejected"
        ? { label: "Rejected", cls: "bg-pill-pro-bg text-pill-pro-fg" }
        : { label: "Pending review", cls: "bg-pill-warn-bg text-pill-warn-fg" };

  return (
    <section
      data-proposal-card={proposal.id}
      aria-label={`Proposed change set ${proposal.id}`}
      className={`overflow-hidden rounded-card border bg-surface ${
        isPending ? "border-blue/45 ring-1 ring-blue/20" : "border-hairline"
      }`}
    >
      <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/60 px-3.5 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          <GitPullRequest className="size-4 shrink-0 text-blue" strokeWidth={1.5} aria-hidden />
          <span className="truncate text-xs font-medium text-ink">Proposed Studio Changes</span>
        </div>
        <div className="flex flex-wrap items-center gap-1.5">
          <Pill className="bg-board px-1.5 py-0 font-mono text-xs capitalize text-ink-2">
            {proposal.mode || "plan"}
          </Pill>
          <Pill className={`px-2 py-0 text-xs font-medium ${statusTone.cls}`}>{statusTone.label}</Pill>
        </div>
      </div>

      <div className="flex flex-col gap-2.5 p-3.5">
        <p className="text-xs leading-relaxed text-ink [overflow-wrap:anywhere]">{proposal.summary}</p>

        <div className="flex flex-wrap items-center gap-1.5">
          <Pill className="bg-board px-2 py-0.5 font-mono text-xs tabular-nums text-ink-2">
            {proposal.file_diffs.length} {proposal.file_diffs.length === 1 ? "script" : "scripts"}
          </Pill>
          {instancesAdded > 0 && (
            <Pill className="bg-tag-mint-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-mint-fg">
              +{instancesAdded} parts
            </Pill>
          )}
          <Pill className="bg-pill-active-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-active-fg">
            +{totalAdded}
          </Pill>
          <Pill className="bg-pill-pro-bg px-2 py-0.5 font-mono text-xs tabular-nums text-pill-pro-fg">
            -{totalRemoved}
          </Pill>
          <Pill
            className={`px-2 py-0.5 font-mono text-xs ${
              errCount > 0 ? "bg-pill-pro-bg text-pill-pro-fg" : "bg-pill-active-bg text-pill-active-fg"
            }`}
          >
            {errCount > 0 ? `${errCount} err` : "0 Luau err"}
          </Pill>
        </div>

        {proposal.file_diffs.length > 0 && (
          <div className="flex flex-wrap items-center gap-1.5">
            {proposal.file_diffs.map((fd) => (
              <Button
                key={fd.path}
                variant="outline"
                size="xs"
                onClick={() => onInspect(proposal.id, fd.path)}
                aria-label={`Inspect diff for ${fd.path}`}
                className="gap-1.5 bg-board/55 font-mono text-xs max-md:min-h-11"
              >
                <FileDiff className="size-3 text-icon" strokeWidth={1.5} aria-hidden />
                <span className="max-w-[11rem] truncate">{fd.path.split(".").pop() || fd.path}</span>
                <span className="text-pill-active-fg">+{fd.additions}</span>
                <span className="text-pill-pro-fg">-{fd.deletions}</span>
              </Button>
            ))}
          </div>
        )}

        <div className="flex flex-wrap items-center gap-1.5 border-t border-hairline pt-2.5">
          <Button
            variant="outline"
            size="xs"
            onClick={() => onInspect(proposal.id, proposal.file_diffs[0]?.path)}
            aria-label={`Inspect diff for proposal ${proposal.id}`}
            className="bg-surface max-md:min-h-11"
          >
            <FileDiff className="size-3.5" strokeWidth={1.5} aria-hidden />
            Inspect Diff
          </Button>
          {isPending && (
            <>
              <Button
                variant="default"
                size="xs"
                disabled={busy}
                onClick={() => onApply(proposal.id)}
                aria-label={`Accept and apply proposal ${proposal.id}`}
                className="max-md:min-h-11"
              >
                <Check className="size-3.5" strokeWidth={2} aria-hidden />
                Accept &amp; Apply
              </Button>
              <Button
                variant="outline"
                size="xs"
                disabled={busy}
                onClick={() => onReject(proposal.id)}
                aria-label={`Reject proposal ${proposal.id}`}
                className="bg-surface text-pill-pro-fg max-md:min-h-11"
              >
                <X className="size-3.5" strokeWidth={1.5} aria-hidden />
                Reject
              </Button>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

function TraceStagePill({ label, done }: { label: string; done: boolean }) {
  return (
    <span
      className={`inline-flex h-6 items-center gap-1 rounded-chip px-2 text-xs font-medium ${
        done ? "bg-pill-active-bg text-pill-active-fg" : "bg-track text-ink-3"
      }`}
    >
      {done && <Check className="size-3" strokeWidth={2} aria-hidden />}
      {label}
    </span>
  );
}

function VerdictBanner({
  tone,
  raw,
  consoleText,
}: {
  tone: "ok" | "bad" | "warn";
  raw: string;
  consoleText?: string;
}) {
  const cfg = {
    ok: {
      badge: "Passed",
      pillCls: "bg-pill-active-bg text-pill-active-fg",
      bandCls: "bg-pill-active-bg/45",
      Icon: Check,
      summary: "The Studio playtest ran through with zero Luau errors.",
    },
    bad: {
      badge: "Failed",
      pillCls: "bg-pill-pro-bg text-pill-pro-fg",
      bandCls: "bg-pill-pro-bg/55",
      Icon: X,
      summary: plain(raw),
    },
    warn: {
      badge: "Not proven yet",
      pillCls: "bg-pill-warn-bg text-pill-warn-fg",
      bandCls: "bg-pill-warn-bg/55",
      Icon: AlertTriangle,
      summary: "The playtest output could not be verified, so this build counts as unproven.",
    },
  }[tone];

  return (
    <div className={`border-t border-hairline px-4 py-3 ${cfg.bandCls}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          <Pill className={`px-2.5 py-0.5 text-xs font-medium ${cfg.pillCls}`}>
            <cfg.Icon className="size-3.5" strokeWidth={2} aria-hidden />
            <span>{cfg.badge}</span>
          </Pill>
          <span className="text-xs font-medium text-ink">Playtest verdict</span>
        </div>
      </div>

      <p className="mt-1.5 text-sm text-ink [overflow-wrap:anywhere]">{cfg.summary}</p>

      <Expandable expandDirection="vertical" expandBehavior="push">
        {({ isExpanded }) => (
          <div className="mt-2">
            <ExpandableTrigger
              aria-label={isExpanded ? "Hide Luau console log" : "Show Luau console log"}
              className="inline-flex min-h-11 items-center gap-1.5 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-surface/70 hover:text-ink md:min-h-7"
            >
              <Terminal className="size-3.5" strokeWidth={1.5} aria-hidden />
              <span>Luau console log</span>
              <ChevronDown
                className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                strokeWidth={1.5}
                aria-hidden
              />
            </ExpandableTrigger>
            <ExpandableContent preset="fade">
              <pre className="mt-1.5 max-h-44 overflow-auto whitespace-pre-wrap break-all rounded-sm border border-hairline bg-surface p-2.5 font-mono text-xs text-ink">
                {raw}
                {consoleText ? `\n${consoleText}` : "\n[0 Luau runtime errors]"}
              </pre>
            </ExpandableContent>
          </div>
        )}
      </Expandable>
    </div>
  );
}

export function ThreadFeed({
  lines,
  notices,
  pending,
  ideas,
  builtTitles,
  canBuild,
  busy,
  buildingIdea,
  freshFrom,
  lastTriedIdea,
  buildPhase,
  latestFileDiffs = [],
  proposals = [],
  capturedFrameUrl,
  projectId,
  onPrefillPrompt,
  onQuickCreatorHubSetup,
  onOpenShip,
  onBuild,
  onOpenScriptDiff,
  onInspectProposal,
  onApplyProposal,
  onRejectProposal,
}: {
  lines: string[];
  notices: { text: string; bad?: boolean; user?: boolean }[];
  pending: { message?: string; build?: string; undo?: boolean } | undefined;
  ideas: Idea[];
  builtTitles: string[];
  canBuild: boolean;
  busy: boolean;
  buildingIdea: string | undefined;
  freshFrom: number;
  lastTriedIdea: Idea | null;
  buildPhase?: BuildStreamPhase | null;
  latestFileDiffs?: ScriptFileDiff[];
  proposals?: PendingProposal[];
  capturedFrameUrl?: string | null;
  projectId?: string | null;
  onPrefillPrompt?: (prompt: string) => void;
  onQuickCreatorHubSetup?: () => Promise<CreatorHubSetupResult | void>;
  onOpenShip?: () => void;
  onBuild: (idea: Idea) => void;
  onOpenScriptDiff?: (path: string) => void;
  onInspectProposal?: (proposalId: string, scriptPath?: string) => void;
  onApplyProposal?: (proposalId: string) => void;
  onRejectProposal?: (proposalId: string) => void;
}) {
  // Find first user turn so pre-build pitch lines do not duplicate the starter workbench.
  const firstUserIdx = lines.findIndex((l) => l.startsWith("You: "));
  const activeLines = firstUserIdx >= 0 ? lines.slice(firstUserIdx) : [];
  const items = activeLines.map(classify);

  // Count traces so we attach latestFileDiffs to the final build trace cleanly.
  let totalTraces = 0;
  let inTrace = false;
  for (const it of items) {
    if (it.kind === "step" || it.kind === "verdict" || it.kind === "console") {
      if (!inTrace) {
        totalTraces++;
        inTrace = true;
      }
      if (it.kind === "verdict") inTrace = false;
    } else {
      inTrace = false;
    }
  }

  const out: React.ReactNode[] = [];
  let traceBuffer: Row[] = [];
  let traceSeen = 0;
  let pitchedRun: { title: string; pitch: string }[] = [];

  const flushTrace = (key: string) => {
    if (!traceBuffer.length) return;
    traceSeen++;
    const isLatestTrace = traceSeen === totalTraces;
    out.push(
      <BuildTrace
        key={key}
        rows={traceBuffer}
        fileDiffs={isLatestTrace ? latestFileDiffs : undefined}
        onOpenScriptDiff={onOpenScriptDiff}
      />,
    );
    traceBuffer = [];
  };

  const flushPitched = (key: string) => {
    if (!pitchedRun.length) return;
    const cards = pitchedRun;
    pitchedRun = [];
    out.push(
      <div key={key} data-answers="1" className="grid grid-cols-1 items-stretch gap-2.5">
        {cards.map((it, idx) => {
          const live = ideas.find((x) => x.title === it.title);
          const idea: Idea = live ?? { id: `pitched-${idx}`, title: it.title, pitch: it.pitch, skills: [] };
          const unavailable = live ? undefined : builtTitles.includes(it.title) ? "built" : "offer";
          return (
            <BlueprintCard
              key={`${it.title}-${idx}`}
              idea={idea}
              canBuild={canBuild}
              busy={busy}
              building={buildingIdea === it.title}
              unavailable={unavailable}
              featured={idx === 0 && !unavailable}
              onBuild={() => live && onBuild(live)}
            />
          );
        })}
      </div>,
    );
  };

  items.forEach((it, idx) => {
    const absIdx = (firstUserIdx >= 0 ? firstUserIdx : 0) + idx;
    const fresh = absIdx >= freshFrom;
    if (it.kind === "step" || it.kind === "verdict" || it.kind === "console") {
      flushPitched(`pitched-${idx}`);
      traceBuffer.push(it);
      if (it.kind === "verdict") flushTrace(`trace-${idx}`);
      return;
    }
    flushTrace(`trace-${idx}`);
    if (it.kind === "idea") {
      pitchedRun.push({ title: it.title, pitch: it.pitch });
      return;
    }
    flushPitched(`pitched-${idx}`);
    if (it.kind === "user") {
      out.push(<UserPrompt key={`u-${idx}`} text={it.text} fresh={fresh} />);
    } else if (it.kind === "agent") {
      const isBad = FAILURE.test(it.text);
      out.push(
        <AgentMessage
          key={`a-${idx}`}
          text={it.text}
          bad={isBad}
          fresh={fresh}
          projectId={projectId}
          onPrefillPrompt={onPrefillPrompt}
          onQuickCreatorHubSetup={onQuickCreatorHubSetup}
          onOpenShip={onOpenShip}
          onRetry={isBad && lastTriedIdea ? () => onBuild(lastTriedIdea) : undefined}
          retryDisabled={!canBuild || busy}
        />,
      );
    }
  });

  flushTrace("trace-end");
  flushPitched("pitched-end");

  // Filter out duplicate notice if the exact same failure line was already persisted in the server thread.
  const tailAgentText = items.filter((r): r is Extract<Row, { kind: "agent" }> => r.kind === "agent").pop()?.text;
  notices.forEach((n, i) => {
    if (!n.user && tailAgentText && plain(n.text) === tailAgentText) return;
    out.push(
      n.user ? (
        <UserPrompt key={`n-${i}`} text={n.text} fresh />
      ) : (
        <AgentMessage
          key={`n-${i}`}
          text={n.text}
          bad={n.bad}
          whole
          fresh
          projectId={projectId}
          onPrefillPrompt={onPrefillPrompt}
          onQuickCreatorHubSetup={onQuickCreatorHubSetup}
          onOpenShip={onOpenShip}
          onRetry={n.bad && lastTriedIdea ? () => onBuild(lastTriedIdea) : undefined}
          retryDisabled={!canBuild || busy}
        />
      ),
    );
  });

  // Render pending or recently resolved RoPilot proposals inline in the Copilot feed
  if (proposals.length > 0 && onInspectProposal && onApplyProposal && onRejectProposal) {
    for (const prop of proposals.slice(0, 4)) {
      out.push(
        <ProposalDiffCard
          key={`prop-${prop.id}`}
          proposal={prop}
          busy={busy}
          onInspect={onInspectProposal}
          onApply={onApplyProposal}
          onReject={onRejectProposal}
        />,
      );
    }
  }

  if (capturedFrameUrl) {
    out.push(
      <figure
        key="captured-frame"
        className="overflow-hidden rounded-card border border-hairline bg-surface p-2"
      >
        <figcaption className="mb-1.5 flex items-center justify-between text-xs text-ink-2">
          <span>Latest Roblox Studio capture</span>
          <span className="font-mono text-xs text-ink-3">Live viewport</span>
        </figcaption>
        <img
          src={capturedFrameUrl}
          alt="Latest captured Roblox Studio viewport frame"
          className="block max-h-36 w-full rounded-sm border border-hairline object-cover"
        />
      </figure>,
    );
  }

  if (pending?.message) {
    out.push(<UserPrompt key="pending-msg" text={pending.message} fresh />);
  }
  if (pending) {
    out.push(
      <WorkingBanner
        key="working"
        phase={pending.undo ? null : buildPhase}
        label={
          pending.build
            ? `Building ${pending.build} in Roblox Studio and running playtest…`
            : pending.undo
              ? "Reverting the last Studio build…"
              : "Generating Luau and preparing Studio changes…"
        }
      />,
    );
  }

  return <div className="flex flex-col gap-3">{out}</div>;
}
