import { useCallback, useEffect, useRef, useState } from "react";
import { read, post, RELEASES_URL } from "./data";
import { Key } from "./Key";

export type Snap = {
  current?: string; latest?: string; available?: boolean; mode?: string; can_apply?: boolean;
  dev_build?: boolean; staged?: string | null; notes?: string; release_url?: string | null;
  error?: string | null; checked_at?: number | null;
};

const DISMISS_KEY = "openro-update-dismissed";
export const isLocalHost = () => ["localhost", "127.0.0.1", "[::1]"].includes(location.hostname);

/** The server's raw update errors become one plain sentence; the raw text sits behind Details. */
export function plainUpdateError(raw: string): string {
  if (/could not reach|Connection/.test(raw)) return "Could not reach GitHub. Check your internet connection, then try again.";
  if (/checksum/.test(raw)) return "The download did not pass its safety check, so it was not installed. Nothing changed. Try again in a few minutes.";
  if (/answered HTTP/.test(raw)) return "GitHub had a problem. Nothing changed. Try again in a few minutes.";
  if (/only allowed from this machine/.test(raw)) return "Updating works only on the computer OpenRO runs on. Open it there to update.";
  if (/does not run on this machine/.test(raw)) return "The new version does not run on this computer, so it was not installed. Nothing changed.";
  return "The update did not finish. Nothing changed. Try again in a few minutes.";
}

function checkedWords(secs: number) {
  const mins = Math.round((Date.now() - secs * 1000) / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins} min ago`;
  return `${Math.round(mins / 60)} h ago`;
}

export function resultLine(s: Snap): string {
  if (s.mode === "off") return "Update checks are turned off on this OpenRO.";
  if (s.staged) return `Version ${s.staged} is installed. Restart to use it.`;
  if (s.available && s.latest) return `Version ${s.latest} is available.`;
  if (!s.checked_at) return s.current ? `OpenRO ${s.current} is running.` : "Not checked yet.";
  return `You are on the newest version (checked ${checkedWords(s.checked_at)}).`;
}

/** Update state and actions, as the old page had them (COMMON.md, update banner). */
export function useUpdate() {
  const [snap, setSnap] = useState<Snap | null>(null);
  const [dismissed, setDismissed] = useState(() => { try { return localStorage.getItem(DISMISS_KEY) || ""; } catch { return ""; } });
  const [busy, setBusy] = useState<"" | "apply" | "restart" | "check">("");
  const [error, setError] = useState<{ plain: string; raw: string; retry: () => void } | null>(null);
  const [checkNote, setCheckNote] = useState("");
  const [returning, setReturning] = useState(false);

  const load = useCallback(() => { read<Snap>("/api/update").then(setSnap).catch(() => {}); }, []);
  useEffect(() => {
    load();
    const t = setInterval(load, 30 * 60 * 1000);
    return () => clearInterval(t);
  }, [load]);

  const check = async () => {
    setBusy("check"); setError(null); setCheckNote("Checking…");
    try {
      const s = await post<Snap & { error?: string }>("/api/update/check", {});
      setSnap(s); setCheckNote(s.error ? "" : resultLine(s));
    } catch (e) {
      setCheckNote(plainUpdateError(String(e)));
    }
    setBusy("");
  };

  const apply = async () => {
    setBusy("apply"); setError(null);
    try {
      const data = await post<Snap & { ok?: boolean; error?: string }>("/api/update/apply", {});
      if (data.ok === false) throw new Error(data.error || "");
      setSnap((s) => ({ ...s, ...data, staged: data.staged || data.latest || s?.latest }));
    } catch (e) {
      const raw = e instanceof Error ? e.message : String(e);
      setError({ plain: plainUpdateError(raw), raw, retry: apply });
    }
    setBusy("");
  };

  const restart = async () => {
    setBusy("restart"); setError(null);
    try {
      const data = await post<{ ok?: boolean; error?: string }>("/api/update/restart", {});
      if (data.ok === false) throw new Error(data.error || "");
    } catch (e) {
      // A dropped connection is the restart itself; any other failure is shown.
      if (!(e instanceof TypeError)) {
        const raw = e instanceof Error ? e.message : String(e);
        setError({ plain: raw || "OpenRO could not restart.", raw, retry: restart });
        setBusy("");
        return;
      }
    }
    // Poll for the new process for up to a minute, then reload on it.
    setReturning(true);
    const started = Date.now();
    const tick = async () => {
      if (Date.now() - started > 60000) { setReturning(false); setBusy(""); setError({ plain: "OpenRO did not come back within a minute. Start OpenRO again, then reload this page.", raw: "", retry: restart }); return; }
      try { const r = await fetch("/api/version", { cache: "no-store" }); if (r.ok) { location.reload(); return; } } catch { /* still restarting */ }
      setTimeout(tick, 1000);
    };
    setTimeout(tick, 1000);
  };

  const dismiss = (latest: string) => {
    try { localStorage.setItem(DISMISS_KEY, latest); } catch { /* the dismissal lasts this visit */ }
    setDismissed(latest);
  };
  const showAgain = () => { try { localStorage.removeItem(DISMISS_KEY); } catch { /* ignore */ } setDismissed(""); };

  const show = !!snap && (!!snap.staged || (!!snap.available && !!snap.latest && dismissed !== snap.latest));
  return { snap, busy, error, checkNote, returning, check, apply, restart, dismiss, showAgain, show, dismissed, canCheck: isLocalHost() && snap?.mode !== "off" };
}

/** The strip above the thread: result, one action, plain errors, focus kept on the line that changed. */
export function UpdateStrip({ u, version }: { u: ReturnType<typeof useUpdate>; version: string }) {
  const statusRef = useRef<HTMLParagraphElement>(null);
  const restartRef = useRef<HTMLButtonElement>(null);
  const showRef = useRef<HTMLButtonElement>(null);
  const [focusNext, setFocusNext] = useState<"status" | "restart" | "show" | "">("");
  useEffect(() => {
    if (focusNext === "status") statusRef.current?.focus();
    if (focusNext === "restart") restartRef.current?.focus();
    if (focusNext === "show") showRef.current?.focus();
    if (focusNext) setFocusNext("");
  }, [focusNext]);

  const s = u.snap;
  const local = isLocalHost();
  const staged = s?.staged;
  const title = staged ? `Version ${staged} is installed. Restart to use it.` : `A new version of OpenRO is ready: ${s?.latest ?? ""}`;
  const hint = s?.dev_build
    ? "This is a developer copy, so it does not update itself. Install a released version to get updates."
    : "To update, open OpenRO on the computer it runs on.";

  return (
    <>
      <div className="flex min-h-11 flex-wrap items-center gap-x-3 gap-y-1 px-4 text-sm text-ink-2 md:px-6">
        {u.canCheck && (
          <button type="button" onClick={() => void u.check()} disabled={u.busy !== ""} className="inline-flex min-h-11 cursor-pointer items-center rounded-sm px-1 text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus disabled:cursor-default">
            Check for updates
          </button>
        )}
        <span className="min-w-0 break-words" role="status" aria-live="polite">{u.checkNote}</span>
        {!u.show && u.dismissed && s?.available && (
          <button ref={showRef} type="button" onClick={() => { u.showAgain(); setFocusNext("status"); }} className="inline-flex min-h-11 cursor-pointer items-center rounded-sm px-1 text-ink underline-offset-2 outline-none hover:underline focus-visible:ring-2 focus-visible:ring-focus">Show update</button>
        )}
        {s?.release_url || version ? (
          <a href={s?.release_url || `${RELEASES_URL}${version}`} target="_blank" rel="noopener" className="ml-auto inline-flex min-h-11 items-center text-ink-2 outline-none hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">Release notes<span className="sr-only"> (opens in a new tab)</span></a>
        ) : null}
      </div>
      {u.show && s && (
        <section aria-label="OpenRO update" className="mx-4 mb-3 flex flex-col gap-2 rounded-card card-stock p-4 md:mx-6">
          <p ref={statusRef} tabIndex={-1} className="font-medium outline-none">{u.busy === "apply" ? "Downloading and checking..." : u.busy === "restart" || u.returning ? "Restarting…" : title}</p>
          {!staged && !u.error && (
            <p className="text-sm text-ink-2">{s.can_apply && local ? "Update now downloads it, checks it, and replaces this app. Restart to start using it." : hint}</p>
          )}
          {s.notes && (
            <details className="text-sm text-ink-2">
              <summary className="inline-flex min-h-11 cursor-pointer items-center outline-none hover:text-ink">What's new</summary>
              <p className="whitespace-pre-wrap break-words">{s.notes.split("\n").find((l) => l.trim()) ? s.notes : ""}</p>
            </details>
          )}
          {u.error && (
            <div className="flex flex-col gap-1 text-sm text-pill-pro-fg">
              <p>{u.error.plain}</p>
              {u.error.raw && <details><summary className="cursor-pointer text-ink-2">Details</summary><pre className="whitespace-pre-wrap break-words font-mono text-xs">{u.error.raw}</pre></details>}
            </div>
          )}
          <div className="flex flex-wrap gap-2">
            {staged && (
              <Key ref={restartRef} onClick={() => void u.restart()} disabled={u.busy !== ""}>{u.busy === "restart" ? "Restarting…" : "Restart now"}</Key>
            )}
            {!staged && s.can_apply && local && (
              <Key onClick={() => { setFocusNext("status"); void u.apply(); }} disabled={u.busy !== ""}>{u.busy === "apply" ? "Downloading and checking..." : u.error ? "Try again" : "Update now"}</Key>
            )}
            {!staged && s.latest && (
              <a href={s.release_url || `${RELEASES_URL}${s.latest}`} target="_blank" rel="noopener" className="inline-flex min-h-11 items-center rounded-card border border-hairline bg-surface px-4 text-ui text-ink outline-none hover:bg-raised focus-visible:ring-2 focus-visible:ring-focus">What's new<span className="sr-only"> (opens in a new tab)</span></a>
            )}
            {!staged && (
              <button type="button" onClick={() => { if (s.latest) u.dismiss(s.latest); setFocusNext("show"); }} className="inline-flex min-h-11 cursor-pointer items-center rounded-card px-4 text-ui text-ink-2 outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">Dismiss</button>
            )}
          </div>
        </section>
      )}
    </>
  );
}
