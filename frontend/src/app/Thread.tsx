import { useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { Check, X, AlertTriangle, FileText, Box, Trash2, Sparkles } from "lucide-react";
import { Key } from "./Key";
import { Expandable, ExpandableTrigger, ExpandableContent } from "@/components/ui/expandable";
import { Pill, PillStatus, PillIndicator } from "@/components/kibo-ui/pill";
import { TextShimmer } from "@/components/ui/text-shimmer";
import { BorderTrail } from "@/components/ui/border-trail";
import { ProgressCircle } from "@/components/ProgressCircle/ProgressCircle";
import { press, layout, reveal } from "@/motion";
import { ADDS, agentText, classify, plain, stepInfo, ERROR_LIKE, type Idea, type Row } from "./data";

const ICONS = { doc: FileText, box: Box, trash: Trash2, check: Check } as const;

/** The ask, from the person: an ink card. */
export function UserBubble({ text, fresh }: { text: string; fresh?: boolean }) {
  return (
    <motion.div initial={fresh ? { opacity: 0, y: 4 } : false} animate={{ opacity: 1, y: 0 }} transition={reveal} className="flex justify-end">
      <p className="max-w-[min(84%,560px)] whitespace-pre-wrap break-words rounded-panel rounded-tr-sm bg-ink-button px-4 py-3 text-ui text-ink-button-fg">{text}</p>
    </motion.div>
  );
}

/** OpenRO speaking: a cream card with a sparkle, errors get a plain sentence and Details. */
export function AgentBubble({ text, bad, whole, fresh }: { text: string; bad?: boolean; whole?: boolean; fresh?: boolean }) {
  const { plain: say, raw } = agentText(text, !!bad, !!whole);
  return (
    <motion.div initial={fresh ? { opacity: 0, y: 4 } : false} animate={{ opacity: 1, y: 0 }} transition={reveal} className="flex items-start gap-3">
      <span className="grid size-8 flex-none place-items-center rounded-full bg-blue text-blue-fg shadow-card" aria-hidden><Sparkles className="size-4" strokeWidth={1.5} /></span>
      <div className={`min-w-0 rounded-card border p-4 text-ui shadow-card ${bad ? "border-transparent bg-tag-design-bg text-tag-design-fg" : "border-hairline bg-surface text-ink"}`}>
        <span className="break-words">{say}</span>
        {raw && (
          <details className="mt-2">
            <summary className="cursor-pointer text-sm text-ink-2 hover:text-ink">Details</summary>
            <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-sm text-ink-2">{raw}</pre>
          </details>
        )}
      </div>
    </motion.div>
  );
}

/** One idea, a big card-stock tile: the thing the app says first. */
export function IdeaTile({ idea, canBuild, busy, building, onBuild }: { idea: Idea; canBuild: boolean; busy: boolean; building: boolean; onBuild: () => void }) {
  return (
    <motion.article whileTap={{ scale: 0.99 }} transition={press} className="relative flex min-w-0 flex-col gap-3 overflow-hidden rounded-card border border-hairline bg-surface p-5 shadow-card">
      {building && <BorderTrail className="bg-blue" size={80} />}
      <h3 className="font-heading text-title tracking-display text-ink">{idea.title}</h3>
      <p className="text-ui text-ink-2">{plain(idea.pitch)}</p>
      <Expandable expandDirection="vertical" expandBehavior="push">
        {({ isExpanded }) => (
          <div className="flex flex-col">
            <ExpandableTrigger aria-expanded={isExpanded} className="cursor-pointer self-start rounded-sm text-sm text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-blue">
              What it adds
            </ExpandableTrigger>
            <ExpandableContent preset="fade">
              <ul className="mt-2 flex flex-wrap gap-1.5">
                {[...new Set(idea.skills.map((s) => ADDS[s] || s.replace(/_/g, " ")))].map((t) => (
                  <li key={t}><Pill className="bg-track text-sm text-ink">{t}</Pill></li>
                ))}
                {!idea.skills.length && <li className="text-sm text-ink-2">Nothing extra; the idea is the whole build.</li>}
              </ul>
            </ExpandableContent>
          </div>
        )}
      </Expandable>
      <div className="mt-auto flex flex-col gap-2 pt-1">
        <Key disabled={!canBuild || busy} onClick={onBuild} aria-label={`Build ${idea.title}`} className="w-full">
          Build this
        </Key>
        {!canBuild && <p className="text-sm text-ink-2">Connect Studio to build this. The banner above says how.</p>}
      </div>
    </motion.article>
  );
}

/** The build tray: stamps fill in as steps arrive; the verdict is the last stamp. */
export function BuildTray({ rows, working }: { rows: Row[]; working: boolean }) {
  const steps = rows.filter((r): r is Extract<Row, { kind: "step" }> => r.kind === "step");
  const verdict = rows.find((r): r is Extract<Row, { kind: "verdict" }> => r.kind === "verdict");
  const console_ = rows.find((r): r is Extract<Row, { kind: "console" }> => r.kind === "console");
  const infos = steps.map((s) => stepInfo(s.raw));
  const saved = infos.some((i) => i.stamp === "saved");
  const generated = infos.some((i) => i.stamp === "generated");
  const done = [saved, generated, !!verdict].filter(Boolean).length;
  return (
    <section aria-label="What OpenRO did" className="relative overflow-hidden rounded-card border border-hairline bg-surface shadow-card">
      {working && <BorderTrail className="bg-blue" size={60} />}
      <div className="flex flex-wrap items-center gap-3 border-b border-hairline p-4">
        <ProgressCircle value={Math.round((done / 3) * 100)} radius={16} strokeWidth={4} className="size-10">
          <span className="font-mono text-xs font-medium tabular-nums">{done}/3</span>
        </ProgressCircle>
        <div className="flex min-w-0 flex-1 flex-wrap gap-2">
          <Stamp label="Saved script" done={saved} tone="bg-blue-soft text-ink" />
          <Stamp label="Generated" done={generated} tone="bg-blue-soft text-ink" />
          <Stamp label="Tested" done={!!verdict} tone="bg-blue-soft text-ink" />
          {verdict && <VerdictStamp tone={verdict.tone} />}
        </div>
      </div>
      {working && (
        <p role="status" className="px-4 pt-3 text-sm text-ink-2">
          <TextShimmer as="span" duration={2}>OpenRO is working on it. A build can take a minute or two.</TextShimmer>
        </p>
      )}
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
                    <summary className="cursor-pointer text-sm text-ink-3 hover:text-ink">Details</summary>
                    <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-sm text-ink-2">{s.raw}</pre>
                  </details>
                </div>
                <span className="pt-1 text-sm text-ink-2">{info.result}</span>
              </li>
            );
          })}
        </ol>
      )}
      {verdict && <VerdictBand tone={verdict.tone} raw={verdict.raw} console={console_?.text} />}
    </section>
  );
}

function Stamp({ label, done, tone }: { label: string; done: boolean; tone: string }) {
  return (
    <motion.span layout transition={layout} className={`inline-flex h-8 items-center gap-1.5 rounded-chip px-3 text-sm font-medium ${done ? tone : "bg-track text-ink-3"}`}>
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
  return (
    <div className={`border-t border-hairline p-4 ${tone === "ok" ? "bg-pill-active-bg text-pill-active-fg" : tone === "bad" ? "bg-pill-pro-bg text-pill-pro-fg" : "bg-pill-warn-bg text-pill-warn-fg"}`}>
      <strong className="block font-heading text-ui font-semibold">{copy[0]}</strong>
      <p className="mt-1 text-ui text-ink">{copy[1]}</p>
      <details className="mt-2">
        <summary className="cursor-pointer text-sm text-ink-2 hover:text-ink">Details</summary>
        <pre className="mt-1 whitespace-pre-wrap break-words font-mono text-sm text-ink">{raw}{out ? "\nConsole output from the test run:\n" + out : ""}</pre>
      </details>
    </div>
  );
}

/** The whole thread from the server's lines, in order: ask, agent, ideas, build trays. */
export function ThreadBody({
  lines, notices, pending, ideas, canBuild, busy, buildingIdea, freshFrom, onBuild,
}: {
  lines: string[];
  notices: { text: string; bad?: boolean; user?: boolean }[];
  pending: { message?: string; build?: string } | undefined;
  ideas: Idea[];
  canBuild: boolean;
  busy: boolean;
  buildingIdea: string | undefined;
  freshFrom: number;
  onBuild: (idea: Idea) => void;
}) {
  const items = lines.map(classify);
  const out: ReactNode[] = [];
  let tray: Row[] = [];
  const flushTray = (key: string) => {
    if (tray.length) out.push(<BuildTray key={key} rows={tray} working={false} />);
    tray = [];
  };
  let ideaRun: Idea[] = [];
  const flushIdeas = (key: string) => {
    if (!ideaRun.length) return;
    out.push(
      <div key={key} className="grid grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-4">
        {ideaRun.map((i) => <IdeaTile key={i.id} idea={i} canBuild={canBuild} busy={busy} building={buildingIdea === i.title} onBuild={() => onBuild(i)} />)}
      </div>,
    );
    ideaRun = [];
  };
  items.forEach((it, i) => {
    const fresh = i >= freshFrom;
    if (it.kind === "step" || it.kind === "verdict" || it.kind === "console") {
      flushIdeas("ideas-" + i);
      tray.push(it);
      // a verdict closes its tray; steps that follow start the next one
      if (it.kind === "verdict") flushTray("tray-" + i);
      return;
    }
    flushTray("tray-" + i);
    if (it.kind === "idea") {
      const rec = ideas.find((x) => x.title === it.title);
      if (rec) ideaRun.push(rec);
      return;
    }
    flushIdeas("ideas-" + i);
    if (it.kind === "user") out.push(<UserBubble key={i} text={it.text} fresh={fresh} />);
    else if (it.kind === "agent") out.push(<AgentBubble key={i} text={it.text} bad={ERROR_LIKE.test(it.text)} fresh={fresh} />);
  });
  flushTray("tray-end");
  flushIdeas("ideas-end");
  notices.forEach((n, i) =>
    out.push(n.user ? <UserBubble key={"n" + i} text={n.text} fresh /> : <AgentBubble key={"n" + i} text={n.text} bad={n.bad} whole fresh />),
  );
  if (pending?.message) out.push(<UserBubble key="pending" text={pending.message} fresh />);
  if (pending) {
    out.push(
      <p key="working" role="status" className="text-ui text-ink-2">
        <TextShimmer as="span" duration={2}>OpenRO is working on it. A build can take a minute or two.</TextShimmer>
      </p>,
    );
  }
  return (
    <>
      {out}
    </>
  );
}

export function Welcome({ canBuild }: { canBuild: boolean }) {
  return (
    <section className="flex flex-col gap-3 pb-2 pt-4">
      <h2 className="font-heading text-display tracking-display text-ink">What do you want to make?</h2>
      <p className="max-w-[60ch] text-ui text-ink-2">
        {canBuild
          ? "Pick one of these ideas, or describe your own game in the box below. OpenRO builds it in Roblox Studio and runs a test before it says it is done."
          : "Pick an idea or describe your own game below. To build it, OpenRO needs a model key and a Roblox Studio connection. See the notice above."}
      </p>
      <p className="text-sm text-ink-2">Roblox Studio is where your game lives; OpenRO needs it open to build and test.</p>
    </section>
  );
}
