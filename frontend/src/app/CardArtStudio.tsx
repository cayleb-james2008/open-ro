import { useEffect, useRef, useState, type ChangeEvent, type DragEvent } from "react";
import { toast } from "sonner";
import {
  BookOpen,
  Image as ImageIcon,
  Palette,
  Sparkles,
  Upload,
  Wand2,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { SegmentedControl, SegmentedControlItem } from "@/components/ui/segmented-control";
import { Pill } from "@/components/kibo-ui/pill";
import {
  MorphingDialog,
  MorphingDialogClose,
  MorphingDialogContainer,
  MorphingDialogContent,
  MorphingDialogDescription,
  MorphingDialogTitle,
} from "@/components/ui/morphing-dialog";
import { open as dialogSpring } from "@/motion";
import { post } from "./data";
import {
  ELEMENT_BADGE_CLS,
  RARITY_BADGE_CLS,
  type ArtKnowledgeGuide,
  type CardAssetRecord,
  type CardElement,
  type CardRarity,
} from "./cardCatalog";

type StudioTab = "gallery" | "create" | "guide";

const SELECTABLE_RARITIES: CardRarity[] = ["Rare", "Epic", "Legendary", "Mythic", "Celestial", "Secret"];
const SELECTABLE_ELEMENTS: CardElement[] = ["Flame", "Void", "Storm", "Shadow", "Arcane"];

export function CardArtStudio({
  open,
  onOpenChange,
  assets,
  knowledge,
  v060Ready,
  onAssetAdded,
  onOpenSettings,
}: {
  open: boolean;
  onOpenChange: (v: boolean) => void;
  assets: CardAssetRecord[];
  knowledge: ArtKnowledgeGuide;
  v060Ready: boolean;
  onAssetAdded: (asset: CardAssetRecord) => void;
  onOpenSettings: () => void;
}) {
  const [tab, setTab] = useState<StudioTab>("gallery");
  const [cardTitle, setCardTitle] = useState("Crimson Blade Sovereign");
  const [rarity, setRarity] = useState<CardRarity>("Mythic");
  const [element, setElement] = useState<CardElement>("Flame");
  const [prompt, setPrompt] = useState(
    "Cel-shaded anime trading card portrait, 3:4 vertical, blazing samurai sovereign channeling molten crimson aura sparks, crisp ink linework, zero text or watermarks.",
  );
  const [provider, setProvider] = useState<"gemini" | "openai">("gemini");
  const [busy, setBusy] = useState(false);
  const [genError, setGenError] = useState("");
  const [dropActive, setDropActive] = useState(false);
  const uploadRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (open) setGenError("");
  }, [open]);

  const slugify = (s: string) =>
    s
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "") || `card-${Date.now()}`;

  const uploadImageFile = async (file: File) => {
    if (!/\.(png|jpe?g|webp)$/i.test(file.name)) {
      toast.error("Please select a PNG, JPG, or WebP card illustration.");
      return;
    }
    setBusy(true);
    setGenError("");
    try {
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader();
        reader.onload = () => resolve(String(reader.result || ""));
        reader.onerror = () => reject(new Error("Could not read image file"));
        reader.readAsDataURL(file);
      });
      const base64 = dataUrl.includes(",") ? dataUrl.split(",")[1] : dataUrl;
      const title = cardTitle.trim() || file.name.replace(/\.[^.]+$/, "");
      const cardId = slugify(title);
      const payload = {
        filename: file.name,
        data_base64: base64,
        data_url: dataUrl,
        card_id: cardId,
        title,
        rarity,
        odds: "Custom Upload",
        element,
        atk: rarity === "Secret" ? 660 : rarity === "Celestial" ? 540 : rarity === "Mythic" ? 430 : 320,
        hp: rarity === "Secret" ? 2050 : rarity === "Celestial" ? 1680 : rarity === "Mythic" ? 1460 : 1120,
        speed: 92,
        passive: `${element} Resonance: +18% squad ${element} ATK`,
        ultimate: `${title} Burst (3.0x ${element} DMG)`,
        prompt: prompt.trim(),
        source: "upload" as const,
      };

      if (v060Ready) {
        const res = await post<{ ok?: boolean; error?: string; asset?: Partial<CardAssetRecord>; file?: string; url?: string }>(
          "/api/assets/upload",
          payload,
        );
        if (res.ok === false) {
          setGenError(res.error || "Upload failed on server.");
          return;
        }
        const savedFile = res.file || res.asset?.file || file.name;
        const savedUrl = res.url || res.asset?.image_url || (savedFile ? `/api/assets/${encodeURIComponent(savedFile)}` : dataUrl);
        onAssetAdded({
          ...payload,
          file: savedFile,
          image_url: savedUrl.startsWith("/api/assets/") || savedUrl.startsWith("data:image/") ? savedUrl : dataUrl,
        });
      } else {
        onAssetAdded({
          ...payload,
          file: file.name,
          image_url: dataUrl,
        });
      }
      toast.success(`Added "${title}" to Card Art Studio`);
      setTab("gallery");
    } catch {
      toast.error("Could not upload card art file.");
    } finally {
      setBusy(false);
    }
  };

  const onUploadChange = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (file) void uploadImageFile(file);
  };

  const onDropFile = (e: DragEvent<HTMLDivElement>) => {
    e.preventDefault();
    setDropActive(false);
    const file = e.dataTransfer.files?.[0];
    if (file) void uploadImageFile(file);
  };

  const handleGenerate = async () => {
    if (!prompt.trim() || busy) return;
    setBusy(true);
    setGenError("");
    const title = cardTitle.trim() || "Custom Anime Card";
    const cardId = slugify(title);
    try {
      if (!v060Ready) {
        setGenError(
          "Live image generation requires a Gemini (Nano Banana) or OpenAI Image API key configured in Settings on a running openro-rs 0.6.0 server — or upload any PNG/WebP card art directly above.",
        );
        return;
      }
      const res = await post<{ ok?: boolean; error?: string; asset?: Partial<CardAssetRecord>; url?: string; file?: string }>(
        "/api/assets/generate",
        {
          provider,
          model: provider === "gemini" ? "gemini-nano-banana-2.1" : "gpt-image-1",
          card_id: cardId,
          title,
          rarity,
          element,
          prompt: prompt.trim(),
        },
      );
      if (!res.ok) {
        setGenError(
          res.error ||
            "No image-generation API key is configured. Add GEMINI_API_KEY (Nano Banana) in Settings or upload PNG/WebP art directly.",
        );
        return;
      }
      const file = res.file || res.asset?.file || `${cardId}.png`;
      const url = res.url || res.asset?.image_url || `/api/assets/${encodeURIComponent(file)}`;
      onAssetAdded({
        card_id: cardId,
        title,
        rarity,
        odds: rarity === "Secret" ? "1 in 8,192" : rarity === "Celestial" ? "1 in 2,048" : "1 in 512",
        element,
        atk: rarity === "Secret" ? 680 : rarity === "Celestial" ? 550 : 420,
        hp: rarity === "Secret" ? 2100 : rarity === "Celestial" ? 1700 : 1450,
        speed: 94,
        passive: `${element} Mastery: +20% ${element} squad ATK`,
        ultimate: `${title} Nova (3.2x ${element} DMG)`,
        prompt: prompt.trim(),
        file,
        image_url: url,
        source: provider === "gemini" ? "gemini-api" : "openai-api",
      });
      toast.success(`Generated "${title}" card art`);
      setTab("gallery");
    } catch {
      setGenError("Could not reach /api/assets/generate. Configure an API key in Settings or upload PNG/WebP art.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <MorphingDialog transition={dialogSpring} open={open} onOpenChange={onOpenChange}>
      <MorphingDialogContainer>
        <MorphingDialogContent className="relative mx-3 flex max-h-[88dvh] w-full max-w-[780px] flex-col rounded-dialog border border-hairline bg-surface text-ink shadow-lift">
          <div data-card-art-studio className="flex min-h-0 flex-1 flex-col">
            <input
              ref={uploadRef}
              type="file"
              accept=".png,.jpg,.jpeg,.webp,image/png,image/jpeg,image/webp"
              hidden
              onChange={onUploadChange}
            />

            <div className="flex items-start justify-between gap-3 border-b border-hairline px-5 py-4">
              <div className="min-w-0">
                <div className="flex flex-wrap items-center gap-2">
                  <Palette className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                  <MorphingDialogTitle className="text-title font-medium text-ink">
                    Card Art &amp; Asset Studio
                  </MorphingDialogTitle>
                  <Pill className="bg-tag-design-bg px-2 py-0.5 font-mono text-xs tabular-nums text-tag-design-fg">
                    {assets.length} cards
                  </Pill>
                </div>
                <MorphingDialogDescription className="mt-0.5 text-sm text-ink-2">
                  Inspect Nano Banana card illustrations, upload or generate custom artwork, and browse the Anime Homage Style Guide.
                </MorphingDialogDescription>
              </div>
              <MorphingDialogClose
                aria-label="Close Card Art Studio"
                className="static inline-flex size-11 flex-none cursor-pointer items-center justify-center rounded-lg border border-hairline bg-surface text-ink-2 outline-none transition-[color,background-color,border-color] duration-(--dur-press) ease-(--ease-house) hover:bg-board hover:text-ink md:size-8"
              >
                <X className="size-4" strokeWidth={1.5} aria-hidden />
              </MorphingDialogClose>
            </div>

            <div className="flex flex-wrap items-center justify-between gap-2 border-b border-hairline bg-board/50 px-5 py-2.5">
              <SegmentedControl aria-label="Card Art Studio views" size="sm">
                <SegmentedControlItem
                  data-checked={tab === "gallery" ? "" : undefined}
                  aria-pressed={tab === "gallery"}
                  onClick={() => setTab("gallery")}
                  className="gap-1.5 max-md:!h-11 max-md:px-2.5"
                >
                  <ImageIcon className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Card Gallery
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "create" ? "" : undefined}
                  aria-pressed={tab === "create"}
                  onClick={() => setTab("create")}
                  className="gap-1.5 max-md:!h-11 max-md:px-2.5"
                >
                  <Wand2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Upload &amp; Generate
                </SegmentedControlItem>
                <SegmentedControlItem
                  data-checked={tab === "guide" ? "" : undefined}
                  aria-pressed={tab === "guide"}
                  onClick={() => setTab("guide")}
                  className="gap-1.5 max-md:!h-11 max-md:px-2.5"
                >
                  <BookOpen className="size-3.5" strokeWidth={1.5} aria-hidden />
                  Anime Style Guide
                </SegmentedControlItem>
              </SegmentedControl>

              <Button
                variant="outline"
                size="xs"
                onClick={() => {
                  onOpenChange(false);
                  onOpenSettings();
                }}
                className="bg-surface max-md:min-h-11"
              >
                Image API Settings
              </Button>
            </div>

            <div className="flex min-h-0 flex-1 flex-col gap-4 overflow-y-auto px-5 py-4">
              {tab === "gallery" && (
                <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 lg:grid-cols-4">
                  {assets.map((card) => (
                    <div
                      key={card.card_id}
                      className="flex flex-col overflow-hidden rounded-card border border-hairline bg-board/40"
                    >
                      <div className="relative aspect-[3/4] w-full overflow-hidden bg-board">
                        <img
                          src={card.image_url}
                          alt={`${card.title} (${card.rarity} ${card.element})`}
                          className="h-full w-full object-cover"
                        />
                      </div>
                      <div className="flex flex-1 flex-col gap-1.5 p-2.5">
                        <div className="flex flex-wrap items-center justify-between gap-1">
                          <Badge className={`border-transparent px-1.5 py-0 text-xs font-medium ${RARITY_BADGE_CLS[card.rarity]}`}>
                            {card.rarity}
                          </Badge>
                          <Badge className={`border-transparent px-1.5 py-0 text-xs font-medium ${ELEMENT_BADGE_CLS[card.element]}`}>
                            {card.element}
                          </Badge>
                        </div>
                        <h4 className="truncate text-sm font-medium text-ink" title={card.title}>
                          {card.title}
                        </h4>
                        <div className="flex items-center justify-between font-mono text-xs tabular-nums text-ink-2">
                          <span>ATK {card.atk}</span>
                          <span>HP {card.hp}</span>
                          <span>SPD {card.speed}</span>
                        </div>
                        <p className="line-clamp-2 text-xs text-ink-2">{card.ultimate}</p>
                        <div className="mt-auto flex items-center justify-between border-t border-hairline pt-1.5 text-xs text-ink-3">
                          <span className="font-mono">{card.odds}</span>
                          <span>
                            {card.source === "google-nano-banana" || card.source === "nano-banana"
                              ? "Nano Banana"
                              : card.source === "gemini-api"
                                ? "Gemini API"
                                : card.source === "openai-api"
                                  ? "OpenAI API"
                                  : "Uploaded"}
                          </span>
                        </div>
                      </div>
                    </div>
                  ))}
                </div>
              )}

              {tab === "create" && (
                <div className="flex flex-col gap-4">
                  <div
                    onDragOver={(e) => {
                      e.preventDefault();
                      setDropActive(true);
                    }}
                    onDragLeave={() => setDropActive(false)}
                    onDrop={onDropFile}
                    className={`flex flex-col items-center justify-center gap-2 rounded-card border-2 border-dashed p-5 text-center transition-[border-color,background-color] duration-(--dur-press) ease-(--ease-house) ${
                      dropActive ? "border-blue bg-blue-soft/50" : "border-hairline bg-board/40"
                    }`}
                  >
                    <Upload className="size-5 text-icon" strokeWidth={1.5} aria-hidden />
                    <p className="text-sm font-medium text-ink">Drag &amp; drop PNG, JPG, or WebP card art here</p>
                    <p className="text-xs text-ink-2">
                      Uploaded art binds automatically to Roblox Studio Decals, ImageLabels, and the live Playable Card Battler.
                    </p>
                    <Button
                      variant="outline"
                      size="sm"
                      disabled={busy}
                      onClick={() => uploadRef.current?.click()}
                      className="mt-1 bg-surface max-md:min-h-11"
                    >
                      <Upload className="size-3.5" strokeWidth={1.5} aria-hidden />
                      Choose card image
                    </Button>
                  </div>

                  <div className="flex flex-col gap-3 rounded-card border border-hairline bg-board/40 p-4">
                    <div className="flex flex-wrap items-center justify-between gap-2">
                      <div className="flex items-center gap-2">
                        <Sparkles className="size-4 text-icon" strokeWidth={1.5} aria-hidden />
                        <h4 className="text-sm font-medium text-ink">Card Metadata &amp; Live Image Generator</h4>
                      </div>
                      <SegmentedControl aria-label="Image generation provider" size="sm">
                        <SegmentedControlItem
                          data-checked={provider === "gemini" ? "" : undefined}
                          aria-pressed={provider === "gemini"}
                          onClick={() => setProvider("gemini")}
                          className="max-md:!h-11"
                        >
                          Gemini Nano Banana
                        </SegmentedControlItem>
                        <SegmentedControlItem
                          data-checked={provider === "openai" ? "" : undefined}
                          aria-pressed={provider === "openai"}
                          onClick={() => setProvider("openai")}
                          className="max-md:!h-11"
                        >
                          OpenAI Image
                        </SegmentedControlItem>
                      </SegmentedControl>
                    </div>

                    <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                      <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>Card Title (DMCA-safe original homage)</span>
                        <Input
                          value={cardTitle}
                          onChange={(e) => setCardTitle(e.target.value)}
                          placeholder="e.g. Solar Dragon Ronin"
                          className="bg-surface text-sm"
                        />
                      </label>

                      <div className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                        <span>Element</span>
                        <div className="flex flex-wrap gap-1">
                          {SELECTABLE_ELEMENTS.map((el) => (
                            <Button
                              key={el}
                              variant={element === el ? "default" : "outline"}
                              size="xs"
                              aria-pressed={element === el}
                              onClick={() => setElement(el)}
                              className="max-md:min-h-11"
                            >
                              {el}
                            </Button>
                          ))}
                        </div>
                      </div>
                    </div>

                    <div className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                      <span>Rarity Tier</span>
                      <div className="flex flex-wrap gap-1">
                        {SELECTABLE_RARITIES.map((r) => (
                          <Button
                            key={r}
                            variant={rarity === r ? "default" : "outline"}
                            size="xs"
                            aria-pressed={rarity === r}
                            onClick={() => setRarity(r)}
                            className="max-md:min-h-11"
                          >
                            {r}
                          </Button>
                        ))}
                      </div>
                    </div>

                    <label className="flex flex-col gap-1 text-xs font-medium text-ink-2">
                      <span>Art Prompt (3:4 vertical cel-shaded trading card portrait)</span>
                      <Input
                        value={prompt}
                        onChange={(e) => setPrompt(e.target.value)}
                        aria-label="Card art prompt"
                        className="bg-surface text-xs"
                      />
                    </label>

                    {genError && (
                      <div role="alert" className="rounded-card border border-hairline bg-pill-pro-bg px-3 py-2 text-xs text-pill-pro-fg">
                        {genError}
                      </div>
                    )}

                    <div className="flex flex-wrap items-center justify-end gap-2">
                      <Button
                        variant="default"
                        size="sm"
                        disabled={busy || !prompt.trim()}
                        onClick={() => void handleGenerate()}
                        className="max-md:min-h-11"
                      >
                        <Wand2 className="size-3.5" strokeWidth={1.5} aria-hidden />
                        {busy ? "Generating…" : "Generate with API"}
                      </Button>
                    </div>
                  </div>
                </div>
              )}

              {tab === "guide" && (
                <div className="flex flex-col gap-4">
                  <div className="rounded-card border border-hairline bg-board/50 p-4">
                    <h4 className="text-ui font-medium text-ink">{knowledge.title}</h4>
                    <p className="mt-1 text-sm leading-relaxed text-ink-2">{knowledge.summary}</p>
                    <div className="mt-3 rounded-sm border border-hairline bg-surface p-3 text-xs leading-relaxed text-ink">
                      <strong className="font-medium">Creative Homage Policy: </strong>
                      {knowledge.policy_note}
                    </div>
                  </div>

                  <div className="flex flex-col gap-2">
                    <h4 className="text-sm font-medium text-ink">Art Direction Pillars</h4>
                    <ul className="grid grid-cols-1 gap-2 sm:grid-cols-2">
                      {knowledge.style_pillars.map((p, idx) => (
                        <li
                          key={idx}
                          className="rounded-card border border-hairline bg-board/40 p-3 text-xs leading-relaxed text-ink-2"
                        >
                          {p}
                        </li>
                      ))}
                    </ul>
                  </div>

                  <div className="flex flex-col gap-2">
                    <h4 className="text-sm font-medium text-ink">DMCA-Safe Archetype Translation Table</h4>
                    <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                      {knowledge.archetypes.map((arch) => (
                        <div
                          key={arch.id}
                          className="flex flex-col justify-between gap-2 rounded-card border border-hairline bg-board/40 p-3"
                        >
                          <div className="flex flex-col gap-1">
                            <div className="flex flex-wrap items-center justify-between gap-1.5">
                              <span className="text-xs text-ink-2">{arch.archetype}</span>
                              <Badge className={`border-transparent px-1.5 py-0 text-xs ${ELEMENT_BADGE_CLS[arch.element]}`}>
                                {arch.element}
                              </Badge>
                            </div>
                            <div className="text-sm font-medium text-ink">{arch.homage_title}</div>
                            <p className="text-xs text-ink-2">{arch.visual_cues}</p>
                          </div>
                          <Button
                            variant="outline"
                            size="xs"
                            onClick={() => {
                              setCardTitle(arch.homage_title);
                              setElement(arch.element);
                              setRarity(arch.rarity);
                              setPrompt(arch.prompt_template);
                              setTab("create");
                              toast.success(`Loaded "${arch.homage_title}" prompt template`);
                            }}
                            className="w-full justify-center bg-surface max-md:min-h-11"
                          >
                            Use homage prompt
                          </Button>
                        </div>
                      ))}
                    </div>
                  </div>
                </div>
              )}
            </div>
          </div>
        </MorphingDialogContent>
      </MorphingDialogContainer>
    </MorphingDialog>
  );
}
