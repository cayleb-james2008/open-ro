import { useCallback, useEffect, useRef, useState } from "react";
import { toast } from "sonner";
import { X } from "lucide-react";
import { read, post } from "./data";
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
      setSnap(s);
      if (s.error) toast.error(plainUpdateError(s.error)); else toast.success(resultLine(s));
      setCheckNote(s.error ? "" : resultLine(s));
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

  const show = (!!snap && (!!snap.staged || (!!snap.available && !!snap.latest && dismissed !== snap.latest))) || !!error || busy === "apply" || busy === "restart" || returning;
  return { snap, busy, error, checkNote, returning, check, apply, restart, dismiss, showAgain, show, dismissed, canCheck: isLocalHost() && snap?.mode !== "off" };
}

/** The slim update line: shown only when an update is available, installing, installed or failed (COMMON.md). One line, dismissible. */
export function UpdateStrip({ u }: { u: ReturnType<typeof useUpdate>; version?: string }) {
  const statusRef = useRef<HTMLParagraphElement>(null);
  const restartRef = useRef<HTMLButtonElement>(null);
  const s = u.snap;
  const local = isLocalHost();
  const staged = s?.staged;
  const busy = u.busy === "apply" || u.busy === "restart" || u.returning;
  const failed = !!u.error;
  const text = busy
    ? (u.busy === "restart" || u.returning ? "Restarting…" : "Downloading and checking…")
    : failed ? u.error!.plain
    : staged ? `Version ${staged} is installed. Restart to use it.`
    : `Version ${s?.latest ?? ""} is ready to install.`;
  const canApply = local && !!s?.can_apply && !staged;
  return (
    <section aria-label="OpenRO update" className="mx-4 mt-3 flex min-h-11 items-center gap-2 rounded-card card-stock py-1 pl-4 pr-1 text-sm md:mx-6">
      <p ref={statusRef} tabIndex={-1} role="status" aria-live="polite" className="min-w-0 flex-1 truncate outline-none">{text}</p>
      {!busy && failed && <Key onClick={() => u.error?.retry()}>Try again</Key>}
      {!busy && !failed && staged && <Key ref={restartRef} onClick={() => void u.restart()}>Restart now</Key>}
      {!busy && !failed && !staged && canApply && <Key onClick={() => void u.apply()}>Update now</Key>}
      {!busy && !failed && !staged && !canApply && <span className="shrink-0 text-ink-2">{s?.dev_build ? "Developer copy" : "Update on this computer"}</span>}
      {!busy && !staged && (
        <button type="button" aria-label="Dismiss update" onClick={() => { if (s?.latest) u.dismiss(s.latest); }} className="grid size-11 flex-none cursor-pointer place-items-center rounded-sm text-icon outline-none hover:bg-track hover:text-ink focus-visible:ring-2 focus-visible:ring-focus">
          <X className="size-4" strokeWidth={1.5} aria-hidden />
        </button>
      )}
    </section>
  );
}
