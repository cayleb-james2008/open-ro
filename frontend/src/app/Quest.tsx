// Making a game is a quest: Pick an idea, Describe it, Build, Playtest, Share. The stage is read from the real thread
// (the last "You:" line and what the server wrote after it), so the spine always matches what happened in Studio.
// Reached stages can be revisited; a new build or message brings the quest back to where the work is.
import { useEffect, useState, type ReactNode } from "react";
import { motion } from "motion/react";
import { glide } from "@/motion";
import { SlidingNumber } from "@/components/ui/sliding-number";
import { toast } from "sonner";
import { Check, Gamepad2, Hammer, Lightbulb, PenLine, Share2 } from "lucide-react";
import { ADDS, FAILURE, agentText, classify, plain, stepInfo, type Idea, type Place } from "./data";
import { Key } from "./Key";
import { StageArt } from "./StageArt";
import { WorkingLine } from "./Thread";

const STAGES = [
  { name: "Pick an idea", Icon: Lightbulb },
  { name: "Describe it", Icon: PenLine },
  { name: "Build", Icon: Hammer },
  { name: "Playtest", Icon: Gamepad2 },
  { name: "Share", Icon: Share2 },
];
// An idea's pitch leads with its one-line summary before the colon; the tile shows that, the Describe stage the whole pitch.
const gist = (pitch: string) => { const p = plain(pitch); const i = p.indexOf(": "); return i > 0 ? p.slice(0, i) : p; };
const nameOf = (path: string) => (path.split(".").pop() || path).replace(/([a-z])([A-Z])/g, "$1 $2");

type Attempt = {
  ask: string; title: string | null; changes: string[]; verdict: { tone: "ok" | "bad" | "warn"; raw: string } | null;
  console: string[]; failure: string | null; reply: string | null;
};

// The work after the last thing the person asked for.
function lastAttempt(lines: string[]): Attempt | null {
  let at = -1;
  for (let i = lines.length - 1; i >= 0; i--) if (lines[i].startsWith("You: ")) { at = i; break; }
  if (at < 0) return null;
  const ask = lines[at].slice(5);
  const rows = lines.slice(at + 1).map(classify);
  const agent = rows.flatMap((r) => (r.kind === "agent" ? [r.text] : []));
  const v = rows.find((r) => r.kind === "verdict");
  return {
    ask,
    title: ask.match(/^Build this — (.+)$/)?.[1] ?? null,
    changes: rows.flatMap((r) => (r.kind === "step" && /^(Wrote|Created|Deleted) /.test(r.raw) ? [r.raw] : [])),
    verdict: v && v.kind === "verdict" ? { tone: v.tone, raw: v.raw } : null,
    console: rows.flatMap((r) => (r.kind === "console" ? [r.text] : [])),
    failure: agent.find((t) => FAILURE.test(t)) ?? null,
    reply: agent.find((t) => !/^Building it now/.test(t)) ?? null,
  };
}

function Chips({ items }: { items: string[] }) {
  return <ul className="flex flex-wrap gap-1.5">{items.map((s) => <li key={s} className="rounded-chip bg-track px-2.5 py-1 text-sm text-ink">{s}</li>)}</ul>;
}

function Stamp({ tone, children }: { tone: "ok" | "bad" | "warn"; children: ReactNode }) {
  const cls = tone === "ok" ? "border-dot-green text-success-foreground" : tone === "bad" ? "border-pill-pro-fg bg-pill-pro-bg text-pill-pro-fg" : "border-ink-3 border-dashed text-ink-2";
  return <span className={"inline-flex min-h-8 items-center rounded-chip border-2 bg-track px-3 text-sm " + cls}>{children}</span>;
}

export function Quest({ lines, ideas, builtTitles, place, pending, canBuild, cantBuildWhy, busy, lastBad, onBuild, onDescribe }: {
  lines: string[]; ideas: Idea[]; builtTitles: string[]; place: Place | null | "error";
  pending: { message?: string; build?: string } | undefined; canBuild: boolean; cantBuildWhy: string; busy: boolean;
  lastBad: string | null; onBuild: (idea: Idea) => void; onDescribe: () => void;
}) {
  const [picked, setPicked] = useState<Idea | null>(null);
  const [view, setView] = useState<number | null>(null);
  const [tried, setTried] = useState<Idea | null>(null);
  const build = (idea: Idea) => { setTried(idea); onBuild(idea); };
  const a = lastAttempt(lines);
  // A failed action (the notices clear when a new one starts) keeps the quest on Build with what went wrong.
  const derived = pending ? 2 : picked ? 1 : lastBad ? 2 : !a ? 0 : a.verdict ? 3 : a.changes.length || a.failure ? 2 : 1;
  const reached = derived >= 3 ? 4 : derived;
  const stage = view !== null && view <= reached ? view : derived;
  // New work (a send, a build, a thread change) brings the quest back to where the work is.
  useEffect(() => { setView(null); }, [lines.length, !!pending]);
  useEffect(() => { if (pending) setPicked(null); }, [pending]);

  const parts = place && place !== "error" ? [...Object.keys(place.scripts || {}), ...Object.keys(place.instances || {})] : null;
  const summary = builtTitles.length
    ? `My Roblox game has ${builtTitles.join(", ")}, ${parts?.length ?? 0} pieces in all, built and playtested with OpenRO.`
    : "";
  const fill = (stage / 4) * 80;
  const ideaTitle = pending?.build ?? a?.title ?? null;
  const ideaOf = (title: string | null) => (title ? ideas.find((i) => i.title === title) : undefined);
  const go = (i: number) => { setView(i); if (i === 0) setPicked(null); };
  const why = !canBuild ? cantBuildWhy : busy ? "Wait for OpenRO to finish the current request." : "";

  return (
    <div className="flex shrink-0 flex-col gap-5">
      <nav aria-label="Quest stages" className="pb-1 pt-2">
        <ol className="relative mx-auto grid max-w-[640px] grid-cols-5 gap-1">
          <div aria-hidden className="absolute left-[10%] right-[10%] top-7 h-2 rounded-chip bg-track" />
          <motion.div aria-hidden className="absolute left-[10%] top-7 h-2 rounded-chip bg-blue shadow-[0_2px_0_var(--key-edge)]"
            initial={false} animate={{ width: `${fill}%` }} {...{ transition: glide }} />
          {STAGES.map(({ name, Icon }, i) => {
            const done = i < stage, here = i === stage;
            return (
              <li key={name} className="relative flex flex-col items-center gap-2">
                <button type="button" disabled={i > reached} aria-current={here ? "step" : undefined} aria-label={name + (done ? ", done" : here ? ", current stage" : i > reached ? ", not reached yet" : "")} onClick={() => go(i)}
                  className={"grid size-14 cursor-pointer place-items-center rounded-full border-2 outline-none transition-[background-color,border-color,color] duration-(--dur-swap) focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default "
                    + (here ? "border-blue bg-blue text-blue-fg shadow-[0_4px_0_var(--key-edge)]" : done ? "border-ink bg-ink-button text-ink-button-fg shadow-[0_3px_0_var(--rim)]" : i <= reached ? "border-hairline bg-surface text-ink" : "border-hairline bg-surface text-ink-3")}>
                  {done ? <Check className="size-6" strokeWidth={2.2} aria-hidden /> : <Icon className="size-6" strokeWidth={1.8} aria-hidden />}
                </button>
                <span aria-hidden className={"text-center text-sm leading-tight " + (here ? "font-medium text-ink" : "text-ink-2")}>{name}</span>
              </li>
            );
          })}
        </ol>
      </nav>

      <section aria-labelledby="stage-title" className="card-stock mx-auto flex w-full max-w-[640px] flex-col gap-5 p-5 md:p-8">
        {stage > 0 && <div className="h-28 rounded-card bg-track p-3 md:h-32"><StageArt stage={stage} /></div>}

        {stage === 0 && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">{builtTitles.length ? "Pick what to add next" : "Pick an idea"}</h2>
            <p className="text-ui text-ink-2">{builtTitles.length ? "Each idea adds to the game you have. Or describe your own change in the box below." : "Each one is a whole game OpenRO can build in Roblox Studio. Or describe your own in the box below."}</p>
            {!ideas.length && <p className="text-ui text-ink-2">No ideas right now. Describe your game in the box below and OpenRO builds it.</p>}
            <div className="grid gap-3">
              {ideas.map((idea) => (
                <button key={idea.id} type="button" onClick={() => setPicked(idea)}
                  className="card-stock flex cursor-pointer flex-col gap-2 p-4 text-left outline-none transition-transform duration-(--dur-press) active:translate-y-0.5 active:scale-[0.97] focus-visible:ring-2 focus-visible:ring-focus">
                  <span className="font-heading text-title font-semibold">{idea.title}</span>
                  <span className="text-ui text-ink-2">{gist(idea.pitch)}</span>
                </button>
              ))}
            </div>
          </>
        )}

        {stage === 1 && picked && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">{picked.title}</h2>
            <p className="text-ui text-ink-2">{plain(picked.pitch)}</p>
            <div><p className="mb-2 text-sm text-ink-2">What it adds</p><Chips items={picked.skills.map((s) => ADDS[s] ?? s.replace(/_/g, " "))} /></div>
            {why && <p className="text-sm text-ink-2">{why}</p>}
            <div className="flex flex-wrap items-center justify-between gap-3">
              <button type="button" onClick={() => setPicked(null)} className="min-h-11 cursor-pointer rounded-card px-3 text-ui text-ink-2 outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">Pick another</button>
              <Key onClick={() => build(picked)} disabled={!!why} className="px-6">Build it</Key>
            </div>
          </>
        )}
        {stage === 1 && !picked && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">Describe it</h2>
            {a ? (
              <div className="grid gap-3">
                <p className="self-end justify-self-end rounded-card bg-blue-soft px-3 py-2 text-ui text-ink">{a.ask}</p>
                {a.reply && <p className="text-ui text-ink">{agentText(a.reply, false, true).plain}</p>}
              </div>
            ) : <p className="text-ui text-ink-2">Say what your game is in plain words, like a lava obby with coins. OpenRO answers here and builds it in Studio.</p>}
            <div><Key onClick={onDescribe} className="px-5">Write in the box below</Key></div>
          </>
        )}

        {stage === 2 && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">{pending ? (ideaTitle ? "Building " + ideaTitle : "Working on it") : a?.failure || (lastBad && tried) ? "The build did not finish" : lastBad ? "That did not go through" : ideaTitle ? "Built " + ideaTitle : "Build"}</h2>
            {pending ? (
              <>
                <WorkingLine>OpenRO is working in Studio. A build can take a minute or two, then it runs a playtest.</WorkingLine>
              </>
            ) : (
              <>
                {(lastBad || a?.failure) && (
                  <div role="alert" className="grid gap-2 rounded-card border-2 border-pill-pro-fg bg-pill-pro-bg p-4">
                    <p className="text-ui text-ink">{agentText((lastBad ?? a?.failure)!, true).plain}</p>
                    {agentText((lastBad ?? a?.failure)!, true).raw && <p className="break-words text-sm text-ink-2">{agentText((lastBad ?? a?.failure)!, true).raw}</p>}
                    {(ideaOf(ideaTitle) ?? tried) && <div><Key onClick={() => build((ideaOf(ideaTitle) ?? tried)!)} disabled={!!why} className="px-5">Try again</Key></div>}
                  </div>
                )}
                {!lastBad && a && a.changes.length > 0 && (
                  <>
                    <p className="text-ui text-ink-2"><SlidingNumber value={a.changes.length} /> {a.changes.length === 1 ? "change" : "changes"} made in Studio</p>
                    <ol className="grid gap-1.5 text-ui text-ink">
                      {a.changes.map((c, i) => <li key={i} className="flex items-start gap-2"><Check className="mt-1 size-4 flex-none text-success-foreground" strokeWidth={2} aria-hidden />{stepInfo(c).label}</li>)}
                    </ol>
                  </>
                )}
              </>
            )}
          </>
        )}

        {stage === 3 && a?.verdict && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">Playtest</h2>
            <div className="flex flex-col items-start gap-3">
              <Stamp tone={a.verdict.tone}>{a.verdict.tone === "ok" ? "Passed" : a.verdict.tone === "bad" ? "Failed" : "Not proven yet"}</Stamp>
              <p className="text-ui text-ink-2">{a.verdict.tone === "ok" ? "The test run played through with no errors." : a.verdict.tone === "bad" ? plain(a.verdict.raw) : "The test result could not be read, so this build counts as unproven."}</p>
              {a.console.length > 0 && <pre className="max-h-40 w-full overflow-auto whitespace-pre-wrap break-words rounded-card bg-track p-3 font-mono text-sm text-ink">{a.console.join("\n")}</pre>}
            </div>
            {parts && parts.length > 0 && <div><p className="mb-2 text-sm text-ink-2">In your game now</p><Chips items={parts.map(nameOf)} /></div>}
            <div className="flex justify-end"><Key onClick={() => setView(4)} className="px-6">Continue</Key></div>
          </>
        )}

        {stage === 4 && (
          <>
            <h2 id="stage-title" className="font-heading text-display tracking-display">Share</h2>
            {builtTitles.length ? (
              <>
                <p className="text-ui text-ink-2">What you have built so far:</p>
                <ul className="flex flex-wrap gap-2">{builtTitles.map((t) => <li key={t}><Stamp tone="ok">{t}</Stamp></li>)}</ul>
                {parts && <p className="text-sm text-ink-2"><SlidingNumber value={parts.length} /> {parts.length === 1 ? "piece" : "pieces"} in your game</p>}
                <p className="rounded-card bg-track p-3 text-ui text-ink">{summary}</p>
              </>
            ) : <p className="text-ui text-ink-2">Nothing built yet. Build an idea first and it shows here.</p>}
            <div className="flex flex-wrap justify-between gap-3">
              {summary && <Key onClick={() => navigator.clipboard.writeText(summary).then(() => toast.success("Copied the game summary"), () => toast.error("Could not copy. Select the text instead."))} className="px-5">Copy game summary</Key>}
              <button type="button" onClick={() => go(0)} className="min-h-11 cursor-pointer rounded-card px-3 text-ui text-ink-2 outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">Pick what to add next</button>
            </div>
          </>
        )}
      </section>
    </div>
  );
}
