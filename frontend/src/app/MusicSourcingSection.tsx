import { useCallback, useEffect, useState } from "react";
import { toast } from "sonner";
import { Download, Music, Search } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Pill, PillIndicator } from "@/components/kibo-ui/pill";
import { call } from "./ship/api";

export type MusicGenre = { id: string; label: string };

export type MusicTrack = {
  id?: string;
  title: string;
  channel?: string;
  duration_s?: number;
  youtube_url?: string;
  url?: string;
  genre?: string;
  license_status?: string;
  credit_line?: string;
};

export type MusicGenresResponse = { ok?: boolean; genres?: Array<{ id?: string; label?: string; name?: string }> };
export type MusicSearchResponse = { ok?: boolean; genre?: string; tracks?: MusicTrack[]; results?: MusicTrack[] };
export type MusicImportResponse = {
  ok?: boolean;
  path?: string;
  filename?: string;
  bytes?: number;
  duration_s?: number;
  sample_rate?: number;
  title?: string;
  channel?: string;
  credit_line?: string;
  license_status?: string;
};
export type MusicRunnerResponse = { ok?: boolean; path?: string; install_hint?: string };

/** Roblox-trending genres; ids match the server's `music_genres()`. Dubstep is never offered. */
const DEFAULT_GENRES: MusicGenre[] = [
  { id: "jazz-rap", label: "Jazz Rap" },
  { id: "lofi", label: "Lo-Fi" },
  { id: "trap", label: "Trap" },
  { id: "carti-type-beat", label: "Carti Type Beat" },
  { id: "pluggnb", label: "PluggnB" },
  { id: "anime-hiphop", label: "Anime Hip-Hop" },
  { id: "synthwave", label: "Synthwave" },
];

const LICENSE_LABELS: Record<string, string> = {
  royalty_free_verified: "Royalty Free",
  free_for_profit: "Free for Profit",
  streambeats_dmca_free: "StreamBeats DMCA-Free",
  non_profit_only: "Non-Profit Only",
  verify_description: "Verify Description",
};

const NO_DUBSTEP = /dubstep|brostep|riddim|wobble/i;

export function formatTrackDuration(seconds?: number): string {
  if (typeof seconds !== "number" || !Number.isFinite(seconds) || seconds < 0) return "--:--";
  const total = Math.round(seconds);
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}

function licenseLabel(status?: string): string {
  if (!status) return "Verify Description";
  return LICENSE_LABELS[status] ?? status.replace(/_/g, " ");
}

function trackUrl(track: MusicTrack): string {
  return (track.youtube_url || track.url || "").trim();
}

function suggestCueName(genre: string): string {
  return genre === "synthwave" || genre === "trap" || genre === "carti-type-beat" ? "battle_music" : "menu_music";
}

export function MusicSourcingSection({
  open,
  projectId,
  onTrackAdded,
}: {
  open: boolean;
  projectId?: string | null;
  onTrackAdded?: () => void;
}) {
  const [genres, setGenres] = useState<MusicGenre[]>(DEFAULT_GENRES);
  const [genreId, setGenreId] = useState<string>(DEFAULT_GENRES[0].id);
  const [query, setQuery] = useState("");
  const [searchBusy, setSearchBusy] = useState(false);
  const [tracks, setTracks] = useState<MusicTrack[]>([]);
  const [searched, setSearched] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [cueNames, setCueNames] = useState<Record<string, string>>({});
  const [importBusy, setImportBusy] = useState<string | null>(null);
  const [importResult, setImportResult] = useState<MusicImportResponse | null>(null);
  const [runnerBusy, setRunnerBusy] = useState(false);
  const [runnerReady, setRunnerReady] = useState(false);

  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    void (async () => {
      const res = await call<MusicGenresResponse>("/api/library/music/genres");
      if (cancelled || !res.ok || !Array.isArray(res.genres)) return;
      const known = new Set(DEFAULT_GENRES.map((g) => g.id));
      const extra = res.genres
        .map((g) => ({ id: String(g.id ?? "").trim(), label: String(g.label ?? g.name ?? g.id ?? "").trim() }))
        .filter((g) => g.id && !known.has(g.id) && !NO_DUBSTEP.test(`${g.id} ${g.label}`));
      if (extra.length > 0) setGenres([...DEFAULT_GENRES, ...extra]);
    })();
    return () => {
      cancelled = true;
    };
  }, [open]);

  const handleSearch = useCallback(async () => {
    setSearchBusy(true);
    setNote(null);
    const res = await call<MusicSearchResponse>("/api/library/music/search", {
      genre: genreId,
      ...(query.trim() ? { query: query.trim() } : {}),
      limit: 8,
    });
    setSearchBusy(false);
    setSearched(true);
    if (!res.ok) {
      setTracks([]);
      const msg =
        res.httpStatus === 404
          ? "Copyright-free music search is available on the desktop OpenRO server (v1.34.0+)."
          : res.error || "Music search failed. Check yt-dlp and your connection, then try again.";
      setNote(msg);
      return;
    }
    const list = Array.isArray(res.tracks) ? res.tracks : Array.isArray(res.results) ? res.results : [];
    setTracks(list.filter((t) => !NO_DUBSTEP.test(`${t.title} ${t.channel ?? ""}`)));
  }, [genreId, query]);

  const handleImport = async (track: MusicTrack) => {
    const url = trackUrl(track);
    if (!url) return;
    const cue = (cueNames[url] ?? "").trim() || suggestCueName(track.genre || genreId);
    setImportBusy(url);
    const res = await call<MusicImportResponse>("/api/library/music/import", {
      youtube_url: url,
      genre: track.genre || genreId,
      title: track.title,
      channel: track.channel,
      license_status: track.license_status,
      credit_line: track.credit_line,
      out_name: cue,
      ...(projectId ? { project: projectId } : {}),
    });
    setImportBusy(null);
    if (!res.ok) {
      const msg =
        res.httpStatus === 404
          ? "Importing music requires the desktop OpenRO server (v1.34.0+)."
          : res.error || `Could not import ${track.title}.`;
      setNote(msg);
      toast.error(msg);
      return;
    }
    setImportResult(res);
    toast.success(`Imported ${res.filename || cue} to project`);
    onTrackAdded?.();
  };

  const handleInstallRunner = async () => {
    setRunnerBusy(true);
    setNote(null);
    const res = await call<MusicRunnerResponse>("/api/library/music-runner/install", {});
    setRunnerBusy(false);
    if (!res.ok) {
      const msg =
        res.httpStatus === 404
          ? "Music Runner installer is available on the desktop server (openro-rs v1.34.0+)."
          : res.error || "Could not install Music Runner.";
      setNote(msg);
      toast.info(msg);
      return;
    }
    setRunnerReady(true);
    setNote(res.install_hint || "Music Runner installed. It needs yt-dlp and ffmpeg on your PATH.");
    toast.success("Installed Music Runner (openro-yt-music)");
  };

  return (
    <section
      data-music-section
      className="flex flex-col gap-2.5 rounded-card border border-hairline bg-board/45 p-3.5"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <div className="flex min-w-0 flex-1 flex-col gap-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Music className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
            <h3 className="text-sm font-medium text-ink">Copyright-Free Music (YouTube)</h3>
            {runnerReady && (
              <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs text-pill-active-fg">
                <PillIndicator variant="success" />
                Runner Ready
              </Pill>
            )}
          </div>
          <p className="text-xs leading-relaxed text-ink-2">
            Pick a genre, find no-copyright tracks, and import one as a 44.1 kHz WAV into your project. Dubstep automatically filtered out.
          </p>
        </div>
        <Button
          variant="outline"
          size="xs"
          disabled={runnerBusy}
          data-install-music-runner
          onClick={() => void handleInstallRunner()}
          className="bg-surface max-md:min-h-11"
        >
          <Download className="size-3" strokeWidth={1.5} aria-hidden />
          <span>{runnerBusy ? "Installing…" : "Install Music Runner"}</span>
        </Button>
      </div>

      <div data-music-genres role="group" aria-label="Music genres" className="flex flex-wrap items-center gap-1.5">
        {genres.map((g) => (
          <Button
            key={g.id}
            variant={genreId === g.id ? "default" : "outline"}
            size="xs"
            aria-pressed={genreId === g.id}
            data-music-genre={g.id}
            onClick={() => setGenreId(g.id)}
            className={genreId === g.id ? "max-md:min-h-11" : "bg-surface max-md:min-h-11"}
          >
            {g.label}
          </Button>
        ))}
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void handleSearch();
            }
          }}
          aria-label="Filter music search"
          placeholder="Optional filter (boom bap, chill, piano, rage synth...)"
          className="h-9 min-w-[200px] flex-1 bg-surface text-xs max-md:min-h-11"
        />
        <Button
          variant="default"
          size="sm"
          disabled={searchBusy}
          data-music-search
          onClick={() => void handleSearch()}
          className="max-md:min-h-11"
        >
          <Search className="size-3.5" strokeWidth={1.5} aria-hidden />
          <span>{searchBusy ? "Searching…" : "Find Tracks"}</span>
        </Button>
      </div>

      {note && (
        <div
          data-music-note
          className="rounded-sm border border-hairline bg-surface px-3 py-2 text-xs leading-relaxed text-ink-2"
        >
          {note}
        </div>
      )}

      {searched && !note && tracks.length === 0 && (
        <p className="text-xs text-ink-2">No copyright-free tracks matched. Try another genre or filter.</p>
      )}

      {tracks.length > 0 && (
        <div data-music-track-list className="flex flex-col gap-2">
          {tracks.map((track) => {
            const url = trackUrl(track);
            const busy = importBusy === url;
            return (
              <div
                key={url || track.title}
                data-music-track={url}
                className="flex flex-col gap-2 rounded-sm border border-hairline bg-surface p-2.5"
              >
                <div className="flex flex-wrap items-center justify-between gap-2">
                  <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                    <span className="truncate text-xs font-medium text-ink">{track.title}</span>
                    <span className="truncate text-xs text-ink-2">{track.channel || "Unknown producer"}</span>
                  </div>
                  <div className="flex shrink-0 items-center gap-1.5">
                    <span className="font-mono text-xs tabular-nums text-ink-3">{formatTrackDuration(track.duration_s)}</span>
                    <Badge variant="outline" className="px-1.5 py-0.5 text-xs">
                      {licenseLabel(track.license_status)}
                    </Badge>
                  </div>
                </div>
                <div className="flex flex-wrap items-center gap-2">
                  <Input
                    value={cueNames[url] ?? ""}
                    onChange={(e) => setCueNames((prev) => ({ ...prev, [url]: e.target.value }))}
                    aria-label={`Cue name for ${track.title}`}
                    placeholder={suggestCueName(track.genre || genreId)}
                    className="h-8 min-w-[160px] flex-1 font-mono text-xs max-md:min-h-11"
                  />
                  <Button
                    variant="default"
                    size="xs"
                    disabled={busy || !url}
                    data-music-import={url}
                    onClick={() => void handleImport(track)}
                    className="max-md:min-h-11"
                  >
                    <Download className="size-3" strokeWidth={1.5} aria-hidden />
                    <span>{busy ? "Importing…" : "Import to Project WAV"}</span>
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      {importResult && (
        <div
          data-music-import-result
          className="flex flex-col gap-1 rounded-sm border border-hairline bg-surface p-2.5 text-xs"
        >
          <div className="flex flex-wrap items-center justify-between gap-1.5">
            <span className="font-mono font-medium text-ink">{importResult.path || importResult.filename}</span>
            <Pill className="bg-pill-active-bg px-2 py-0.5 text-xs tabular-nums text-pill-active-fg">
              {importResult.sample_rate ?? 44100} Hz PCM16 WAV
            </Pill>
          </div>
          <span className="text-ink-2">
            Credit: {importResult.credit_line || importResult.channel || "See the .source.json receipt next to the WAV"}
          </span>
        </div>
      )}
    </section>
  );
}
