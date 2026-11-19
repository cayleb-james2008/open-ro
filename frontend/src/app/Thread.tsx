import { motion, useReducedMotion } from "motion/react";

/** The live line while a build runs: a pear dot that pulses (it carries the running state; it stops when the work stops). */
export function WorkingLine({ children }: { children: string }) {
  const reduce = useReducedMotion();
  return (
    <p role="status" className="flex items-center gap-2 text-ui text-ink-2">
      <motion.span aria-hidden className="size-2.5 flex-none rounded-full bg-blue" animate={reduce ? undefined : { opacity: [0.35, 1, 0.35] }} transition={{ repeat: Infinity, duration: 1.4 }} />
      {children}
    </p>
  );
}
import { Check, X, AlertTriangle, FileText, Box, Trash2, Sparkles, ChevronDown } from "lucide-react";
import { Key } from "./Key";
import { Expandable, ExpandableTrigger } from "@/components/ui/expandable";
import { Pill } from "@/components/kibo-ui/pill";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { glide, layout, reveal, instant } from "@/motion";
import { ADDS, agentText, classify, FAILURE, plain, stepInfo, type Idea, type Row } from "./data";

const ICONS = { doc: FileText, box: Box, trash: Trash2, check: Check } as const;

/** The ask, from the person: an ink card. */
export function UserBubble({ text, fresh }: { text: string; fresh?: boolean }) {
  return (
    <motion.div initial={fresh ? { opacity: 0, y: 4 } : false} animate={{ opacity: 1, y: 0 }} transition={reveal} className="flex justify-end">
      <p className="max-w-[min(84%,560px)] whitespace-pre-wrap break-words rounded-panel rounded-tr-sm bg-ink-button px-4 py-3 text-ui text-ink-button-fg">{text}</p>
    </motion.div>
  );
}

/** OpenRO speaking: cream card with a sparkle. A failure gets the coral card; information stays cream. */
export function AgentBubble({ text, bad, whole, fresh }: { text: string; bad?: boolean; whole?: boolean; fresh?: boolean }) {
  const { plain: say, raw } = agentText(text, !!bad, !!whole);
  return (
    <motion.div initial={fresh ? { opacity: 0, y: 4 } : false} animate={{ opacity: 1, y: 0 }} transition={reveal} className="flex items-start gap-3">
      <span className="grid size-8 flex-none place-items-center rounded-full bg-blue text-blue-fg shadow-[0_2px_0_var(--key-edge)]" aria-hidden><Sparkles className="size-4" strokeWidth={1.5} /></span>
      <div className={`card-stock min-w-0 p-4 text-ui ${bad ? "border-transparent bg-pill-pro-bg text-pill-pro-fg" : "text-ink"}`}>
        <span className="break-words">{say}</span>
        {raw && (
          <Expandable expandDirection="vertical" expandBehavior="push">
            {({ isExpanded }) => (
              <div className="mt-2">
                <ExpandableTrigger aria-expanded={isExpanded} aria-label={isExpanded ? "Hide details" : "Show details"} className="inline-flex min-h-11 min-w-11 cursor-pointer items-center justify-center rounded-sm px-1 text-sm text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">Details</ExpandableTrigger>
                {isExpanded && <pre className="whitespace-pre-wrap break-words font-mono text-sm text-ink-2">{raw}</pre>}
              </div>
            )}
          </Expandable>
        )}
      </div>
    </motion.div>
  );
}

/** One idea, a big card-stock tile: title, a clamped pitch with More, visible chips, the key pinned to the bottom. */
export function IdeaTile({ idea, canBuild, busy, building, unavailable, onBuild }: { idea: Idea; canBuild: boolean; busy: boolean; building: boolean; unavailable?: "built" | "offer"; onBuild: () => void }) {
  const reduce = useReducedMotion();
  const pitch = plain(idea.pitch);
  return (
    <motion.article
      whileHover={reduce ? undefined : { y: -2 }}
      whileTap={reduce ? undefined : { y: 1 }}
      transition={reduce ? instant : glide}
      className={`card-stock relative flex h-full min-w-0 flex-col gap-3 overflow-hidden p-5 ${building ? "ring-2 ring-blue" : ""}`}
    >
      <h3 className="font-heading text-title tracking-display text-ink">{idea.title}</h3>
      <Expandable expandDirection="vertical" expandBehavior="push">
        {({ isExpanded }) => (
          <div className="flex flex-col items-start gap-1">
            <p className={`text-ui text-ink-2 ${isExpanded ? "" : "line-clamp-4"}`}>{pitch}</p>
            {pitch.length > 160 && (
              <ExpandableTrigger aria-expanded={isExpanded} aria-label={`${isExpanded ? "Show less" : "Read more"} about ${idea.title}`} className="inline-flex min-h-11 cursor-pointer items-center gap-1 rounded-chip border border-hairline bg-surface px-3 text-sm font-medium text-ink outline-none hover:bg-raised focus-visible:ring-2 focus-visible:ring-focus">
                {isExpanded ? "Show less" : "Read more"}<ChevronDown className={`size-4 transition-transform duration-(--dur-swap) ease-(--ease-house) ${isExpanded ? "rotate-180" : ""}`} strokeWidth={1.5} aria-hidden />
              </ExpandableTrigger>
            )}
          </div>
        )}
      </Expandable>
      {idea.skills.length > 0 && (
        <div className="flex flex-col gap-2">
          <span className="text-sm text-ink-2">What it adds</span>
          <ul className="flex flex-wrap gap-1.5">
            {[...new Set(idea.skills.map((s) => ADDS[s] || s.replace(/_/g, " ")))].map((t) => (
              <li key={t}><Pill className="bg-tag-sky-bg text-sm text-tag-sky-fg">{t}</Pill></li>
            ))}
          </ul>
        </div>
      )}
      <div className="mt-auto flex flex-col gap-2 pt-1">
        {unavailable ? (
          <p className="text-sm text-ink-2">{unavailable === "built" ? "Already in your game. The next ideas above are the ones you can build." : "Already in your game, or not on offer now. The next ideas above are the ones you can build."}</p>
        ) : (
          <>
            <Key disabled={!canBuild || busy} onClick={onBuild} aria-label={`Build ${idea.title}`} data-focus-key={`idea:${idea.title}`} className="w-full">Build this</Key>
            {!canBuild && <p className="text-sm text-ink-2">Connect Studio to build this. The banner above says how.</p>}
          </>
        )}
      </div>
    </motion.article>
  );
}

/** The build tray: stamps fill in as steps arrive; the verdict is the last stamp. */
export function BuildTray({ rows, working = false }: { rows: Row[]; working?: boolean }) {
  const steps = rows.filter((r): r is Extract<Row, { kind: "step" }> => r.kind === "step");
  const verdict = rows.find((r): r is Extract<Row, { kind: "verdict" }> => r.kind === "verdict");
  const consoleOut = rows.find((r): r is Extract<Row, { kind: "console" }> => r.kind === "console");
  const infos = steps.map((s) => stepInfo(s.raw));
  const saved = infos.some((i) => i.stamp === "saved");
  const generated = infos.some((i) => i.stamp === "generated");
  const done = [saved, generated, !!verdict].filter(Boolean).length;
  return (
    <section aria-label="What OpenRO did" className={`card-stock overflow-hidden ${working ? "ring-2 ring-blue" : ""}`}>
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline p-4">
        <ProgressCircle value={Math.round((done / 3) * 100)} radius={16} strokeWidth={4} className="size-10">
          <span className="font-mono text-xs font-medium tabular-nums">{done}/3</span>
        </ProgressCircle>
        <div className="flex min-w-0 flex-1 flex-wrap gap-2">
          <Stamp label="Saved script" done={saved} />
          <Stamp label="Generated" done={generated} />
          <Stamp label="Tested" done={!!verdict} />
          {verdict && <VerdictStamp tone={verdict.tone} />}
        </div>
      </div>
      {steps.length > 0 && (
        <ol className="m-0 list-none p-0">
          {steps.map((s, i) => {
            const info = infos[i];
            const Icon = ICONS[info.icon];
            return (
              <li key={i} className="grid grid-cols-[28px_minmax(0,1fr)_auto] items-start gap-3 border-t border-hairline px-4 py-3">
                <span className="grid size-7 place-items-center rounded-sm bg-track text-icon"><Icon className="size-4" strokeWidth={1.5} aria-hidden /></span>
                <div className="min-w-0">
                  <div className="break-words text-ui text-ink">{info.label}</div>
                  <details className="mt-1">
                    <summary className="inline-flex min-h-11 cursor-pointer items-center text-sm text-ink-3 hover:text-ink">Details</summary>
                    <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-sm text-ink-2">{s.raw}</pre>
                  </details>
                </div>
                <span className="pt-1 text-sm text-ink-2">{info.result}</span>
              </li>
            );
          })}
        </ol>
      )}
      {verdict && <VerdictBand tone={verdict.tone} raw={verdict.raw} console={consoleOut?.text} />}
    </section>
  );
}

function Stamp({ label, done }: { label: string; done: boolean }) {
  return (
    <motion.span layout transition={layout} className={`inline-flex h-8 items-center gap-1.5 rounded-chip px-3 text-sm font-medium ${done ? "bg-blue-soft text-ink" : "bg-track text-ink-3"}`}>
      {done && <Check className="size-3.5" strokeWidth={2} aria-hidden />}
      {label}
    </motion.span>
  );
}

function VerdictStamp({ tone }: { tone: "ok" | "bad" | "warn" }) {
  const m = {
    ok: { label: "Passed", cls: "bg-pill-active-bg text-pill-active-fg", Icon: Check },
    bad: { label: "Did not pass", cls: "bg-pill-pro-bg text-pill-pro-fg", Icon: X },
    warn: { label: "Not proven yet", cls: "bg-pill-warn-bg text-pill-warn-fg", Icon: AlertTriangle },
  }[tone];
  return (
    <motion.span layout transition={layout} initial={{ scale: 0.92 }} animate={{ scale: 1 }} className={`inline-flex h-8 items-center gap-1.5 rounded-chip px-3 text-sm font-medium ${m.cls}`}>
      <m.Icon className="size-3.5" strokeWidth={2} aria-hidden />
      {m.label}
    </motion.span>
  );
}

function VerdictBand({ tone, raw, console: out }: { tone: "ok" | "bad" | "warn"; raw: string; console?: string }) {
  const copy = {
    ok: ["Passed", "The test run played through with no errors."],
    bad: ["Failed", "The test run found a problem. OpenRO is reading the error and repairing your game."],
    warn: ["Not proven yet", "The test run result could not be read, so this build counts as unproven. OpenRO will test again."],
  }[tone];
  const cls = tone === "ok" ? "bg-pill-active-bg text-pill-active-fg" : tone === "bad" ? "bg-pill-pro-bg text-pill-pro-fg" : "bg-pill-warn-bg text-pill-warn-fg";
  return (
    <div className={`border-t border-hairline p-4 ${cls}`}>
      <strong className="block font-heading text-ui font-semibold">{copy[0]}</strong>
      <p className="mt-1 text-ui text-ink">{copy[1]}</p>
      <details className="mt-2">
        <summary className="inline-flex min-h-11 cursor-pointer items-center text-sm text-ink-2 hover:text-ink">Details</summary>
        <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-sm text-ink">{raw}{out ? "\nConsole output from the test run:\n" + out : ""}</pre>
      </details>
    </div>
  );
}

/** The thread from the server's lines, in order: ask, agent, ideas, build trays. */
export function ThreadBody({
  lines, notices, pending, ideas, builtTitles, canBuild, busy, buildingIdea, freshFrom, onBuild,
}: {
  lines: string[];
  notices: { text: string; bad?: boolean; user?: boolean }[];
  pending: { message?: string; build?: string } | undefined;
  ideas: Idea[];
  builtTitles: string[];
  canBuild: boolean;
  busy: boolean;
  buildingIdea: string | undefined;
  freshFrom: number;
  onBuild: (idea: Idea) => void;
}) {
  const items = lines.map(classify);
  const out: React.ReactNode[] = [];
  let tray: Row[] = [];
  let ideaRun: React.ReactNode[] = [];
  const flushTray = (key: string) => { if (tray.length) out.push(<BuildTray key={key} rows={tray} />); tray = []; };
  const flushIdeas = (key: string) => {
    if (!ideaRun.length) return;
    out.push(<div key={key} className="grid grid-cols-[repeat(auto-fit,minmax(220px,1fr))] items-stretch gap-4">{ideaRun}</div>);
    ideaRun = [];
  };
  items.forEach((it, i) => {
    const fresh = i >= freshFrom;
    if (it.kind === "step" || it.kind === "verdict" || it.kind === "console") {
      flushIdeas("ideas-" + i);
      tray.push(it);
      if (it.kind === "verdict") flushTray("tray-" + i);
      return;
    }
    flushTray("tray-" + i);
    if (it.kind === "idea") {
      const rec = ideas.find((x) => x.title === it.title);
      const idea: Idea = rec ?? { id: "", title: it.title, pitch: it.pitch, skills: [] };
      ideaRun.push(
        <IdeaTile key={"i" + i} idea={idea} canBuild={canBuild} busy={busy} building={buildingIdea === it.title} unavailable={rec ? undefined : builtTitles.includes(it.title) ? "built" : "offer"} onBuild={() => rec && onBuild(rec)} />,
      );
      return;
    }
    flushIdeas("ideas-" + i);
    if (it.kind === "user") out.push(<UserBubble key={i} text={it.text} fresh={fresh} />);
    else if (it.kind === "agent") out.push(<AgentBubble key={i} text={it.text} bad={FAILURE.test(it.text)} fresh={fresh} />);
  });
  flushTray("tray-end");
  flushIdeas("ideas-end");
  notices.forEach((n, i) =>
    out.push(n.user ? <UserBubble key={"n" + i} text={n.text} fresh /> : <AgentBubble key={"n" + i} text={n.text} bad={n.bad} whole fresh />),
  );
  if (pending?.message) out.push(<UserBubble key="pending" text={pending.message} fresh />);
  if (pending) {
    out.push(
      <WorkingLine key="working">OpenRO is working on it. A build can take a minute or two.</WorkingLine>,
    );
  }
  return <>{out}</>;
}
