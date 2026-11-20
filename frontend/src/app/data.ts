// Data layer: the same /api routes and reply shapes the vanilla page used (git show HEAD:assets/index.html).
export type Chat = { id: string; title: string };
export type Idea = { id: string; title: string; pitch: string; skills: string[] };
export type Place = { scripts: Record<string, string>; instances: Record<string, unknown> };
export type ChatList = { chats: Chat[]; active: string; can_undo: boolean };

export const RELEASES_URL = "https://github.com/cayleb-james2008/open-ro/releases/tag/v";

export async function read<T>(url: string): Promise<T> {
  const r = await fetch(url, { cache: "no-store" });
  if (!r.ok) throw new Error(`${url} answered ${r.status}`);
  return (await r.json()) as T;
}

export async function post<T = any>(url: string, body: unknown = {}): Promise<T> {
  const r = await fetch(url, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify(body),
  });
  return (await r.json()) as T;
}

// Thread lines are plain text from the server. These mirror the old page's classify() and stepInfo().
const IDEA_PREFIX = "OpenRO: Idea — ";
const CONSOLE_PREFIX = "Console from the failed test run: ";
const DASH = /\s—\s/g;
export const plain = (s: string) => s.replace(DASH, ", ");
// Thread lines the server writes for a failure (src/server.rs: "OpenRO: {error}" and "OpenRO: {f.message}").
// The built-in opening line ("Building it now ... no model call involved") is information, not a failure.
export const FAILURE = /^(?:OpenRO: )?(?:could not|did not|not connected|studio not connected|studio bridge error|no model key|revert did not|chat not found|empty message|unknown idea|.*\b(?:failed|refused)\b)/i;

export type Row =
  | { kind: "user"; text: string }
  | { kind: "idea"; title: string; pitch: string }
  | { kind: "agent"; text: string }
  | { kind: "console"; text: string }
  | { kind: "verdict"; tone: "ok" | "bad" | "warn"; raw: string }
  | { kind: "step"; raw: string };

export function classify(line: string): Row {
  if (line.startsWith("You: ")) return { kind: "user", text: line.slice(5) };
  if (line.startsWith(IDEA_PREFIX)) {
    const rest = line.slice(IDEA_PREFIX.length);
    const i = rest.indexOf(": ");
    return { kind: "idea", title: i < 0 ? rest : rest.slice(0, i), pitch: i < 0 ? "" : rest.slice(i + 2) };
  }
  if (line.startsWith("OpenRO: ")) return { kind: "agent", text: plain(line.slice(8)) };
  if (line.startsWith(CONSOLE_PREFIX)) return { kind: "console", text: line.slice(CONSOLE_PREFIX.length) };
  if (line === "Playtest passed.") return { kind: "verdict", tone: "ok", raw: line };
  if (line.startsWith("Playtest failed")) return { kind: "verdict", tone: "bad", raw: line };
  if (line.startsWith("Playtest result was not")) return { kind: "verdict", tone: "warn", raw: line };
  return { kind: "step", raw: line };
}

const CLASSES: Record<string, string> = { SpawnLocation: "spawn point", Part: "block" };
export function stepInfo(raw: string): { icon: "doc" | "box" | "trash" | "check"; label: string; result: string; stamp?: "saved" | "generated" } {
  let m = raw.match(/^Wrote (.+)\.$/);
  if (m) return { icon: "doc", label: "Saved the script " + m[1].split(".").pop(), result: "Saved", stamp: "saved" };
  m = raw.match(/^Generated the (.+) script\.$/);
  if (m) return { icon: "doc", label: "Wrote the " + m[1].replace(/_/g, " ") + " code", result: "Written", stamp: "generated" };
  if (raw.startsWith("Generated the ")) return { icon: "doc", label: plain(raw), result: "Written", stamp: "generated" };
  m = raw.match(/^Created (.+) \((.+)\)\.$/);
  if (m) return { icon: "box", label: "Added " + m[1].split(".").pop() + " (" + (CLASSES[m[2]] || m[2].toLowerCase()) + ")", result: "Added" };
  m = raw.match(/^Deleted (.+)\.$/);
  if (m) return { icon: "trash", label: "Removed " + m[1].split(".").pop(), result: "Removed" };
  return { icon: "check", label: plain(raw), result: "Done" };
}

export const ADDS: Record<string, string> = {
  spawn_link: "Spawn point", checkpoints: "Checkpoints", kill_brick: "Lava that burns",
  currency: "Coins that save", day_night: "Day and night", shop: "A shop", game_gui: "A game screen",
  round_manager: "Rounds", leaderboard: "Leaderboard", soundtrack: "Music",
};

// Server errors that are config problems: the plain sentence shows, the raw line sits behind Details.
const PLAIN_ERRORS = [
  { re: /^no model key/, say: "Chat is off: no model key is set, so OpenRO cannot answer. Ask whoever runs OpenRO to add a model key, then restart it." },
  { re: /^Studio bridge error/, say: "Roblox Studio refused that change, so OpenRO stopped and added nothing. Send it again to retry, or ask for something smaller." },
  { re: /^Studio not connected/, say: "Studio is not connected, so nothing can be built. Open Roblox Studio with the OpenRO connection running, then send again." },
];
export function agentText(text: string, bad: boolean, whole = false): { plain: string; raw: string } {
  const known = PLAIN_ERRORS.find((e) => e.re.test(text));
  if (known) return { plain: known.say, raw: text };
  const cut = text.indexOf(" (Details: ");
  if (cut >= 0) return { plain: text.slice(0, cut), raw: text.slice(cut + 11, -1) };
  if (whole) return { plain: text, raw: "" };
  const sentence = text.match(/^(.+?[.!?])\s+(.+)$/s);
  if (bad && sentence) return { plain: sentence[1], raw: sentence[2] };
  if (bad && /:\s/.test(text)) return { plain: "OpenRO could not finish that step. The details are below.", raw: text };
  return { plain: text, raw: "" };
}

export const SEND_FAILED = "That did not go through. Nothing was built. Your message is back in the box.";
