import { useEffect, useState } from "react";
import { toast } from "sonner";
import { Cloud, KeyRound } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill } from "@/components/kibo-ui/pill";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { call, type OpenCloud } from "./api";

/** Settings -> Roblox Open Cloud. The key box is write-only: it never shows a saved key, only the server's mask. */
export function OpenCloudSettings({ onChanged }: { onChanged?: () => void }) {
  const [cloud, setCloud] = useState<OpenCloud | null>(null);
  const [off, setOff] = useState(false);
  const [key, setKey] = useState("");
  const [who, setWho] = useState<"userId" | "groupId">("userId");
  const [num, setNum] = useState("");
  const [busy, setBusy] = useState("");
  const [err, setErr] = useState("");

  const apply = (r: OpenCloud) => {
    setCloud(r);
    setWho(r.creator?.groupId ? "groupId" : "userId");
    setNum(String(r.creator?.groupId ?? r.creator?.userId ?? ""));
    onChanged?.();
  };
  useEffect(() => {
    let live = true;
    void call<OpenCloud>("/api/opencloud").then((r) => {
      if (!live) return;
      if (r.status === 404) setOff(true); else if (r.ok) apply(r as OpenCloud); else setErr(r.error || "Could not read the Open Cloud state.");
    });
    return () => { live = false; };
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const send = async (url: string, body: unknown, label: string, ok: string) => {
    setBusy(label); setErr("");
    const r = await call<OpenCloud>(url, body);
    setBusy("");
    if (!r.ok) { setErr(r.error || "That did not save."); return false; }
    apply(r as OpenCloud); toast.success(ok);
    return true;
  };
  const saveKey = async () => { if (await send("/api/opencloud/key", { api_key: key.trim() }, "key", "Open Cloud key saved")) setKey(""); };
  const saveWho = () => send("/api/opencloud/settings", { creator: { [who]: Number(num.trim()) } }, "who", "Owner saved");

  if (off) return null;
  return (
    <section aria-label="Roblox Open Cloud" className="flex flex-col gap-3 rounded-card border border-hairline bg-board/50 p-3.5" data-opencloud-settings>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2"><Cloud className="size-4 text-icon" strokeWidth={1.5} aria-hidden /><h3 className="text-sm font-medium text-ink">Roblox Open Cloud</h3></div>
        <Pill className={`px-2 py-0.5 text-sm ${cloud?.has_key ? "bg-pill-active-bg text-pill-active-fg" : "bg-pill-inactive-bg text-pill-inactive-fg"}`} data-key-state>{cloud?.has_key ? `Saved ${cloud.key_masked ?? ""}` : "No key yet"}</Pill>
      </div>
      <p className="text-sm text-ink-2">Lets OpenRO upload files, publish and set up game passes for your project. The key is stored on this computer, is never shown again after you save it, and is only used by OpenRO.</p>
      {err && <p role="alert" className="rounded-sm bg-pill-pro-bg px-3 py-2 text-sm text-pill-pro-fg">{err}</p>}
      <label className="flex flex-col gap-1 text-sm font-medium text-ink-2">
        <span className="inline-flex items-center gap-1"><KeyRound className="size-3.5 text-icon" strokeWidth={1.5} aria-hidden />{cloud?.has_key ? "Replace the key" : "Open Cloud API key"}</span>
        <Input type="password" name="openro-opencloud-key" autoComplete="new-password" spellCheck={false} value={key} onChange={(e) => setKey(e.target.value)} placeholder={cloud?.has_key ? "Type a new key to replace it" : "Paste your key"} className="font-mono" aria-describedby="opencloud-scopes" />
      </label>
      <div className="flex flex-wrap gap-2">
        <Button variant="default" size="lg" disabled={!key.trim() || busy === "key"} onClick={() => void saveKey()} className="max-md:min-h-11">{busy === "key" ? "Saving…" : "Save key"}</Button>
        {cloud?.has_key && <Button variant="outline" size="lg" disabled={!!busy} onClick={() => void send("/api/opencloud/key", { clear: true }, "clear", "Open Cloud key removed")} className="max-md:min-h-11">Remove key</Button>}
      </div>
      {cloud?.scopes_needed && cloud.scopes_needed.length > 0 && (
        <p id="opencloud-scopes" className="text-sm text-ink-2">The key needs these permissions: <span className="font-mono">{cloud.scopes_needed.join(", ")}</span></p>
      )}
      <div className="flex flex-col gap-1.5 border-t border-hairline pt-3">
        <span className="text-sm font-medium text-ink-2">Who owns the uploads</span>
        <div className="flex flex-wrap items-center gap-2">
          <SegmentedControl aria-label="Owner kind" size="sm">
            <SegmentedControlItem data-checked={who === "userId" ? "" : undefined} aria-pressed={who === "userId"} onClick={() => setWho("userId")} className="max-md:!h-11 max-md:px-3">Me</SegmentedControlItem>
            <SegmentedControlItem data-checked={who === "groupId" ? "" : undefined} aria-pressed={who === "groupId"} onClick={() => setWho("groupId")} className="max-md:!h-11 max-md:px-3">A group</SegmentedControlItem>
          </SegmentedControl>
          <Input inputMode="numeric" aria-label={who === "userId" ? "Your Roblox user number" : "Group number"} value={num} onChange={(e) => setNum(e.target.value)} placeholder={who === "userId" ? "Your user number" : "Group number"} className="min-w-[140px] flex-1 font-mono" />
          <Button variant="outline" size="lg" disabled={!/^\d+$/.test(num.trim()) || busy === "who"} onClick={() => void saveWho()} className="max-md:min-h-11">Save</Button>
        </div>
      </div>
    </section>
  );
}
