import { motion, useReducedMotion } from "motion/react";
import {
  AlertTriangle,
  Box,
  Check,
  ChevronDown,
  FileCode,
  RefreshCw,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Expandable, ExpandableContent, ExpandableTrigger } from "@/components/ui/expandable";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { Pill, PillIndicator, PillStatus } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { layout, reveal } from "@/motion";
import {
  FAILURE,
  agentText,
  classify,
  plain,
  stepInfo,
  type Idea,
  type Row,
} from "./data";
import { BlueprintCard } from "./Blueprints";

const STEP_ICONS = {
  doc: FileCode,
  box: Box,
  trash: Trash2,
  check: Check,
} as const;

export function WorkingBanner({ label }: { label: string }) {
  const reduce = useReducedMotion();
  return (
    <div
      role="status"
      aria-live="polite"
      className="flex items-center gap-3 rounded-card border border-hairline bg-surface px-4 py-3 text-sm text-ink"
    >
      <Spinner className="size-4 text-blue" />
      <span className="min-w-0 flex-1 [overflow-wrap:anywhere]">{label}</span>
      <Pill className="bg-board text-xs text-ink-2">
        <PillStatus className="border-transparent">
          <PillIndicator variant="info" pulse={!reduce} />
          <span>Running</span>
        </PillStatus>
      </Pill>
    </div>
  );
}

export function UserPrompt({ text, fresh }: { text: string; fresh?: boolean }) {
  return (
    <motion.div
      initial={fresh ? { opacity: 0, y: 4 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={reveal}
      className="flex justify-end"
    >
      <div className="max-w-[min(86%,580px)] rounded-panel rounded-tr-sm border border-hairline bg-ink-button px-4 py-2.5 text-sm leading-relaxed text-ink-button-fg whitespace-pre-wrap [overflow-wrap:anywhere]">
        {text}
      </div>
    </motion.div>
  );
}

export function AgentMessage({
  text,
  bad,
  whole,
  fresh,
  onRetry,
  retryDisabled,
}: {
  text: string;
  bad?: boolean;
  whole?: boolean;
  fresh?: boolean;
  onRetry?: () => void;
  retryDisabled?: boolean;
}) {
  const { plain: say, raw } = agentText(text, !!bad, !!whole);
  return (
    <motion.div
      initial={fresh ? { opacity: 0, y: 4 } : false}
      animate={{ opacity: 1, y: 0 }}
      transition={reveal}
      role={bad ? "alert" : undefined}
      className={`flex flex-col gap-2 rounded-card border p-3.5 text-sm leading-relaxed ${
        bad
          ? "border-pill-pro-fg/35 bg-pill-pro-bg text-pill-pro-fg"
          : "border-hairline bg-surface text-ink"
      }`}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <p className="min-w-0 flex-1 [overflow-wrap:anywhere]">{say}</p>
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

      {raw && (
        <Expandable expandDirection="vertical" expandBehavior="push">
          {({ isExpanded }) => (
            <div className="border-t border-hairline/70 pt-2">
              <ExpandableTrigger
                aria-label={isExpanded ? "Hide message details" : "Show message details"}
                className="inline-flex min-h-11 items-center gap-1 rounded-sm px-2 text-xs font-medium text-ink-2 outline-none hover:bg-board hover:text-ink md:min-h-7"
              >
                <span>Details</span>
                <ChevronDown
                  className={`size-3.5 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`}
                  strokeWidth={1.5}
                  aria-hidden
                />
              </ExpandableTrigger>
              <ExpandableContent preset="fade">
                <pre className="mt-1.5 whitespace-pre-wrap break-all rounded-sm bg-board p-2.5 font-mono text-xs text-ink-2">
                  {raw}
                </pre>
              </ExpandableContent>
            </div>
          )}
        </Expandable>
      )}
    </motion.div>
  );
}

export function BuildTrace({ rows, working = false }: { rows: Row[]; working?: boolean }) {
  const rawSteps = rows.filter((r): r is Extract<Row, { kind: "step" }> => r.kind === "step");
  const verdict = rows.find((r): r is Extract<Row, { kind: "verdict" }> => r.kind === "verdict");
  const consoleRows = rows.filter((r): r is Extract<Row, { kind: "console" }> => r.kind === "console");
  const rawInfos = rawSteps.map((s) => stepInfo(s.raw));
  const changeCount = rawInfos.filter((i) => i.changed).length;
  const generated = rawInfos.some((i) => i.stamp === "generated");
  const applied = rawInfos.some((i) => i.changed);
  const doneStages = [generated || applied, applied, !!verdict].filter(Boolean).length;
  const consoleText = consoleRows.map((c) => c.text).join("\n");

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
      <div className="flex flex-wrap items-center justify-between gap-3 border-b border-hairline bg-board/60 px-4 py-3">
        <div className="flex min-w-0 items-center gap-3">
          <ProgressCircle
            value={Math.round((doneStages / 3) * 100)}
            radius={16}
            strokeWidth={3.5}
            className="size-9 flex-none"
          >
            <span className="font-mono text-xs font-medium tabular-nums text-ink">{doneStages}/3</span>
          </ProgressCircle>

          <div className="min-w-0">
            <div className="inline-flex items-center gap-1.5 whitespace-nowrap text-sm text-ink-2">
              <span className="font-mono font-medium tabular-nums text-ink">
                <SlidingNumber value={changeCount} />
              </span>
              {`${changeCount === 1 ? "change" : "changes"} made in Studio`}
            </div>
          </div>
        </div>

        <div className="flex flex-wrap items-center gap-1.5">
          <TraceStagePill label="Luau" done={generated} />
          <TraceStagePill label="Studio" done={applied} />
          <TraceStagePill label="Playtest" done={!!verdict} />
        </div>
      </div>

      {displaySteps.length > 0 && (
        <ol className="m-0 divide-y divide-hairline list-none p-0">
          {displaySteps.map(({ raw, info }, idx) => {
            const Icon = STEP_ICONS[info.icon];
            return (
              <li key={idx} className="px-4 py-2">
                <Expandable expandDirection="vertical" expandBehavior="push">
                  {({ isExpanded }) => (
                    <>
                      <div className="flex items-start gap-3">
                        <span className="mt-0.5 grid size-6 flex-none place-items-center rounded-sm bg-board text-icon">
                          <Icon className="size-3.5" strokeWidth={1.5} aria-hidden />
                        </span>
                        <div className="min-w-0 flex-1">
                          <div className="flex flex-wrap items-baseline gap-x-2 gap-y-1">
                            <span className="text-sm font-medium text-ink [overflow-wrap:anywhere]">
                              {info.label}
                            </span>
                            {info.target && (
                              <code className="max-w-full rounded-sm bg-board px-1.5 py-0.5 font-mono text-xs text-ink-2 [overflow-wrap:anywhere]">
                                {info.target}
                              </code>
                            )}
                          </div>
                        </div>
                        <div className="flex flex-none items-center gap-1.5">
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

      {verdict && <VerdictBanner tone={verdict.tone} raw={verdict.raw} consoleText={consoleText} />}
    </section>
  );
}

function TraceStagePill({ label, done }: { label: string; done: boolean }) {
  return (
    <motion.span
      layout
      transition={layout}
      className={`inline-flex h-6 items-center gap-1 rounded-chip px-2 text-xs font-medium ${
        done ? "bg-pill-active-bg text-pill-active-fg" : "bg-track text-ink-3"
      }`}
    >
      {done && <Check className="size-3" strokeWidth={2} aria-hidden />}
      {label}
    </motion.span>
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
  onBuild,
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
  onBuild: (idea: Idea) => void;
}) {
  // Find first user turn so pre-build pitch lines do not duplicate the starter workbench.
  const firstUserIdx = lines.findIndex((l) => l.startsWith("You: "));
  const activeLines = firstUserIdx >= 0 ? lines.slice(firstUserIdx) : [];
  const items = activeLines.map(classify);

  const out: React.ReactNode[] = [];
  let traceBuffer: Row[] = [];
  let pitchedRun: { title: string; pitch: string }[] = [];

  const flushTrace = (key: string) => {
    if (!traceBuffer.length) return;
    out.push(<BuildTrace key={key} rows={traceBuffer} />);
    traceBuffer = [];
  };

  const flushPitched = (key: string) => {
    if (!pitchedRun.length) return;
    const cards = pitchedRun;
    pitchedRun = [];
    out.push(
      <div key={key} data-answers="1" className="grid grid-cols-1 items-stretch gap-3 lg:grid-cols-3">
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
          onRetry={n.bad && lastTriedIdea ? () => onBuild(lastTriedIdea) : undefined}
          retryDisabled={!canBuild || busy}
        />
      ),
    );
  });

  if (pending?.message) {
    out.push(<UserPrompt key="pending-msg" text={pending.message} fresh />);
  }
  if (pending) {
    out.push(
      <WorkingBanner
        key="working"
        label={
          pending.build
            ? `Building ${pending.build} in Roblox Studio and running playtest…`
            : pending.undo
              ? "Reverting the last Studio build…"
              : "Generating Luau and applying changes in Roblox Studio…"
        }
      />,
    );
  }

  return <div className="flex flex-col gap-4">{out}</div>;
}
