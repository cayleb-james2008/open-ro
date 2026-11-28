//! OpenRO UI Kit Pipeline (Gap #80, Gap #91):
//! - 9 Dissected AAA UI Craft Rules (incl. #8 image-gen panel coverage, #9 dense master-detail taste)
//! - 4 Mixable Layout Topologies (`skybridge-crest`, `perimeter-command`, `workshop-ribbon`, `compact-mobile-split`)
//! - 5 Original Visual Skins (`obsidian-prism`, `skybridge-gilded`, `sakura-lacquer`, `cyber-forge`, `clean-monochrome`)
//! - 5 Cohesive UI Asset Sheet Recipes (`chrome_sheet`, `icons_sheet`, `rarity_frames_sheet`, `card_borders_sheet`, `panels_sheet`)
//! - Single-line `character_art = false` ChatGPT Image Gen prompts + deterministic 2x Pillow `procedural_fallback`
//! - Auto-slicer integration (`image_ops::slice_ui_sheet`) -> `art/ui/<skin>/*.png` + `art/ui/slices.json`

use crate::assets::{append_art_log, art_generate};
use crate::gui_designer::{save_project_screen, ui_kit_screen_doc};
use crate::image_ops::{render_procedural_ui_sheet, slice_ui_sheet, update_slices_json};
use crate::opencloud::{now_iso, safe_rel, sha256_hex, Oc};
use crate::secrets::SecretStore;
use crate::tool_adapters::{maybe_invoke_override, InvokeCtx};
use serde_json::{json, Value};
use std::path::Path;

#[derive(Clone, Debug)]
pub struct UiCraftRule {
    pub id: &'static str,
    pub number: u8,
    pub title: &'static str,
    pub rule: &'static str,
    pub roblox_binding: &'static str,
    pub anti_pattern: &'static str,
}

#[derive(Clone, Debug)]
pub struct LayoutTopologySpec {
    pub id: &'static str,
    pub name: &'static str,
    pub summary: &'static str,
    pub anchor_zones: &'static [&'static str],
    pub mobile_safe: bool,
}

#[derive(Clone, Debug)]
pub struct SkinTokens {
    pub bg: &'static str,
    pub surface: &'static str,
    pub surface_elevated: &'static str,
    pub text: &'static str,
    pub text_muted: &'static str,
    pub accent: &'static str,
    pub accent_fg: &'static str,
    pub rim: &'static str,
    pub stroke_dark: &'static str,
    pub danger: &'static str,
    pub danger_fg: &'static str,
    pub crest_fill: &'static str,
    pub crest_trim: &'static str,
}

impl SkinTokens {
    pub fn to_json(&self) -> Value {
        json!({
            "bg": self.bg,
            "surface": self.surface,
            "surface_elevated": self.surface_elevated,
            "text": self.text,
            "text_muted": self.text_muted,
            "accent": self.accent,
            "accent_fg": self.accent_fg,
            "rim": self.rim,
            "stroke_dark": self.stroke_dark,
            "danger": self.danger,
            "danger_fg": self.danger_fg,
            "crest_fill": self.crest_fill,
            "crest_trim": self.crest_trim,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UiSkinSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub mood: &'static str,
    pub chroma_key: &'static str,
    pub dark: SkinTokens,
    pub light: SkinTokens,
}

#[derive(Clone, Debug)]
pub struct SheetItemSpec {
    pub name: &'static str,
    pub row: u8,
    pub col: u8,
    pub is_9slice: bool,
    pub cutout_interior: bool,
    pub target_size: [u32; 2],
    pub prompt_desc: &'static str,
}

#[derive(Clone, Debug)]
pub struct UiSheetRecipe {
    pub id: &'static str,
    pub name: &'static str,
    pub cols: u8,
    pub rows: u8,
    pub items: Vec<SheetItemSpec>,
}

pub fn ui_craft_rules() -> Vec<UiCraftRule> {
    vec![
        UiCraftRule {
            id: "illustrated-9-slice-chrome",
            number: 1,
            title: "Illustrated 9-Slice Chrome",
            rule: "Back modal panels, header crests, CTA buttons, progress bars, and rarity frames with sliced transparent PNGs using Enum.ScaleType.Slice and exact SliceCenter Rect.new bounds.",
            roblox_binding: "ImageLabel / ImageButton with ScaleType = Enum.ScaleType.Slice and SliceCenter = AssetIds.slices[key].slice",
            anti_pattern: "Bare flat unstyled Frame boxes for primary modals or buttons.",
        },
        UiCraftRule {
            id: "modal-lighting-blur",
            number: 2,
            title: "Modal Depth via Lighting.BlurEffect",
            rule: "Pair modal screens with a subtle scrim and Lighting.BlurEffect (Size 14-18) toggled on open and cleaned up on close so modal art pops off the 3D scene.",
            roblox_binding: "ScreenDoc.modal_blur = true -> Lighting BlurEffect + Destroying cleanup",
            anti_pattern: "Floating opaque panels directly over noisy 3D foliage/FX without depth separation.",
        },
        UiCraftRule {
            id: "glass-rim-contrast",
            number: 3,
            title: "Obsidian / Alabaster Glass + Chromatic Rim Glow",
            rule: "Use deep obsidian (#0A0D14) in Dark mode and crisp alabaster (#F7F8FA) in Light mode (Settings-only toggle) with >=5.25:1 text contrast and subtle chromatic UIStroke rims.",
            roblox_binding: "BackgroundTransparency = 0.08..0.18 + UIStroke (Thickness 1.5, semantic rim Color)",
            anti_pattern: "Low-contrast grey-on-slate text or standalone HUD light/dark toggle buttons.",
        },
        UiCraftRule {
            id: "original-header-crests",
            number: 4,
            title: "Original Architectural Header Crests",
            rule: "Crown modal and HUD headers with original architectural crest geometry (kite-wing, chamfered architrave, forged bracket, or lacquer eave) tailored to the game world.",
            roblox_binding: "HeaderCrest ImageLabel with ScaleType.Slice overlapping top panel seam",
            anti_pattern: "Copied ANRX crimson 4-point-star ribbons or any franchise crest.",
        },
        UiCraftRule {
            id: "glossy-buttons-outlined-numerals",
            number: 5,
            title: "Glossy Metallic Buttons + Outlined Numerals",
            rule: "Render beveled CTA buttons (>=44x44px) and pair tabular counters/titles with 1.5-2px dark UIStroke outlines so numerals stay legible over bright VFX.",
            roblox_binding: "TextLabel + child UIStroke (Thickness = 1.8, ApplyStrokeMode = Contextual)",
            anti_pattern: "Sub-44px buttons or thin un-outlined numbers over particle bursts.",
        },
        UiCraftRule {
            id: "master-detail-reward-stepper",
            number: 6,
            title: "Master-Detail Modals + Reward Burst Strip + Milestone Stepper",
            rule: "Organize inspection screens as left-list / right-detail splits, center horizontal reward burst strips for rolls/drops, and track pity/goals via horizontal milestone steppers.",
            roblox_binding: "ScrollingFrame master list + detail pane + horizontal UIListLayout reward strip",
            anti_pattern: "Single-column walls of tiny text or ungrouped reward popups.",
        },
        UiCraftRule {
            id: "perimeter-mixable-mobile-first",
            number: 7,
            title: "Perimeter HUD + Mixable Layouts + 844x390 Phone-First",
            rule: "Anchor navigation, currency, and action docks to perimeter safe corridors clearing the 58px TopBar and 170px mobile thumbstick/jump corners on 1920x1080, 1024x768, and 844x390.",
            roblox_binding: "ui_kit_screen_doc(layout, skin) verified 0 warnings across all 3 viewports",
            anti_pattern: "Copying reference HUD coordinates verbatim or blocking the 844x390 thumbstick zone.",
        },
        UiCraftRule {
            id: "image-gen-panel-coverage",
            number: 8,
            title: "Image-Gen-First Panel Coverage (>=70% Sliced Chrome)",
            rule: "Generate chrome_sheet + panels_sheet via ChatGPT Image Gen first, then bind sliced transparent PNGs to >=70% of live rendered modals, headers, tabs, buttons, progress bars, and dock tiles (ScaleType.Slice or Stretch) while every slice-backed parent container keeps BackgroundTransparency = 1 so the art, not a flat fill, defines the silhouette.",
            roblox_binding: "SliceChrome / PanelChrome / TitleRibbon ImageLabel (ScaleType = Enum.ScaleType.Slice, SliceCenter = AssetIds.slices[key].slice) inside a parent GuiObject with BackgroundTransparency = 1; verified by studio_verify_ui_craft (panel_coverage_pct >= 65, opaque_parent_violations == 0)",
            anti_pattern: "Opaque flat Frame fills painted behind sliced art, or primary buttons/tabs left as bare TextButtons with no image chrome.",
        },
        UiCraftRule {
            id: "dense-master-detail-taste",
            number: 9,
            title: "Dense Master-Detail + Multi-Column Tile Grids",
            rule: "Lay out inventory, shop, codex, and team modals as dense split-pane Master-Detail (scrolling tile grid left, inspector_card detail right) or multi-column UIGridLayout tile grids with tab_active/tab_inactive filter strips, filling the modal with purposeful content.",
            roblox_binding: "Modal = modal_glass ImageLabel + title_ribbon header + UIGridLayout (CellSize ~ 96-128px, 3-6 columns) master pane + inspector_card detail pane + btn_emerald/btn_gold CTA row",
            anti_pattern: "Dead single-card voids floating in an empty modal, or bare outline-only stacks of thin bordered boxes.",
        },
        UiCraftRule {
            id: "modern-roblox-pop-gradients",
            number: 10,
            title: "Top-Grossing Roblox Pop-Anime Gradients & Bold-Ink Icons",
            rule: "Match top-grossing anime Roblox UI with matte dark dot-matrix panels, vibrant vertical UIGradient header ribbons and pills (electric lime, sky cyan, gold, crimson), thick 2.5-3px black UIStroke comic outlines, breaking-out cartoon icons (cartoon_icons_sheet), colorful icon+label left rail tabs, glossy red [X] buttons, and frameless bold-outlined floating HUD counters + gradient pity bars.",
            roblox_binding: "Skin 'anime-pop-dark' + cartoon_icons_sheet + UIGradient headers/pills + UIStroke (Thickness 2-3px, #05070B) + Blender FileMesh sanctum props/floor via blender_build_map",
            anti_pattern: "Heavy medieval metallic bevel plates, pointy diamond heraldic spikes, or flat block-part arenas.",
        },
    ]
}

pub fn layout_topologies() -> Vec<LayoutTopologySpec> {
    vec![
        LayoutTopologySpec {
            id: "skybridge-crest",
            name: "Skybridge Crest",
            summary: "Top-center architectural crest banner, top-right currency/pity glass pill, upper-left quick-nav rail, bottom-center primary action dock.",
            anchor_zones: &["top-center-crest", "top-right-currency", "upper-left-rail", "bottom-center-dock"],
            mobile_safe: true,
        },
        LayoutTopologySpec {
            id: "perimeter-command",
            name: "Perimeter Command",
            summary: "Top-left commander badge, top-right currency + utility orb cluster, upper-right milestone strip, bottom-center 3-slot command bar.",
            anchor_zones: &["top-left-badge", "top-right-orb", "upper-right-milestone", "bottom-center-command"],
            mobile_safe: true,
        },
        LayoutTopologySpec {
            id: "workshop-ribbon",
            name: "Workshop Ribbon",
            summary: "Top-center slim milestone stepper ribbon, top-left forge/bag quick pill, bottom-center dual-CTA crafting & inspect ribbon.",
            anchor_zones: &["top-center-stepper", "top-left-quick", "bottom-center-ribbon"],
            mobile_safe: true,
        },
        LayoutTopologySpec {
            id: "compact-mobile-split",
            name: "Compact Mobile Split",
            summary: "Phone-first split corner layout: top-left status crest, top-right currency + bag orb, bottom-center compact dual action bar leaving >=65% clear 3D stage.",
            anchor_zones: &["top-left-status", "top-right-currency", "bottom-center-dual"],
            mobile_safe: true,
        },
    ]
}

pub fn visual_skins() -> Vec<UiSkinSpec> {
    vec![
        UiSkinSpec {
            id: "obsidian-prism",
            name: "Obsidian Prism",
            mood: "Deep obsidian glass with refracted cyan-sapphire chromatic rim glow and machined slate crests",
            chroma_key: "#00FF00",
            dark: SkinTokens {
                bg: "#07090F",
                surface: "#0A0D14",
                surface_elevated: "#131926",
                text: "#F4F7FC",
                text_muted: "#B6C2D9",
                accent: "#38BDF8",
                accent_fg: "#08101C",
                rim: "#38BDF8",
                stroke_dark: "#05070B",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#172033",
                crest_trim: "#7DD3FC",
            },
            light: SkinTokens {
                bg: "#EEF2F7",
                surface: "#F7F8FA",
                surface_elevated: "#FFFFFF",
                text: "#0F172A",
                text_muted: "#475569",
                accent: "#0369A1",
                accent_fg: "#FFFFFF",
                rim: "#0284C7",
                stroke_dark: "#0F172A",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#E2E8F0",
                crest_trim: "#0369A1",
            },
        },
        UiSkinSpec {
            id: "skybridge-gilded",
            name: "Skybridge Gilded",
            mood: "Midnight navy-slate glass framed by sunlit brass architrave bevels and amber highlights",
            chroma_key: "#00FF00",
            dark: SkinTokens {
                bg: "#080D18",
                surface: "#0B1220",
                surface_elevated: "#141F33",
                text: "#F8FAFC",
                text_muted: "#CBD5E1",
                accent: "#F59E0B",
                accent_fg: "#0B1220",
                rim: "#FBBF24",
                stroke_dark: "#070B14",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#1B2840",
                crest_trim: "#FCD34D",
            },
            light: SkinTokens {
                bg: "#F1EFEA",
                surface: "#F7F8FA",
                surface_elevated: "#FFFFFF",
                text: "#0F172A",
                text_muted: "#475569",
                accent: "#92400E",
                accent_fg: "#FFFFFF",
                rim: "#B45309",
                stroke_dark: "#1E293B",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#F3EAD8",
                crest_trim: "#92400E",
            },
        },
        UiSkinSpec {
            id: "sakura-lacquer",
            name: "Sakura Lacquer",
            mood: "Plum-obsidian lacquer glass with rose-quartz trim and warm ivory typography",
            chroma_key: "#00FF00",
            dark: SkinTokens {
                bg: "#0D0810",
                surface: "#120B14",
                surface_elevated: "#1F1424",
                text: "#FDF2F8",
                text_muted: "#E2C6D8",
                accent: "#F472B6",
                accent_fg: "#120B14",
                rim: "#F472B6",
                stroke_dark: "#09050B",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#2A1A30",
                crest_trim: "#F9A8D4",
            },
            light: SkinTokens {
                bg: "#F5EEF2",
                surface: "#FAF6F8",
                surface_elevated: "#FFFFFF",
                text: "#1E101A",
                text_muted: "#5B4152",
                accent: "#9D174D",
                accent_fg: "#FFFFFF",
                rim: "#BE185D",
                stroke_dark: "#1E101A",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#F5DFEB",
                crest_trim: "#9D174D",
            },
        },
        UiSkinSpec {
            id: "cyber-forge",
            name: "Cyber Forge",
            mood: "Graphite foundry plates with crisp teal-emerald telemetry edges and machined chamfers",
            chroma_key: "#FF00FF",
            dark: SkinTokens {
                bg: "#060B0D",
                surface: "#090F12",
                surface_elevated: "#111D22",
                text: "#ECFDF5",
                text_muted: "#A7F3D0",
                accent: "#2DD4BF",
                accent_fg: "#090F12",
                rim: "#14B8A6",
                stroke_dark: "#04080A",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#152830",
                crest_trim: "#5EEAD4",
            },
            light: SkinTokens {
                bg: "#EBF2F4",
                surface: "#F4F8F9",
                surface_elevated: "#FFFFFF",
                text: "#0C1A1F",
                text_muted: "#3B525A",
                accent: "#0F766E",
                accent_fg: "#FFFFFF",
                rim: "#0D9488",
                stroke_dark: "#0C1A1F",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#D8ECEF",
                crest_trim: "#0F766E",
            },
        },
        UiSkinSpec {
            id: "clean-monochrome",
            name: "Clean Monochrome",
            mood: "Minimal architectural obsidian-and-alabaster ink chrome with silver hairline bevels",
            chroma_key: "#00FF00",
            dark: SkinTokens {
                bg: "#06080D",
                surface: "#0A0D14",
                surface_elevated: "#141923",
                text: "#F7F8FA",
                text_muted: "#B8C0CC",
                accent: "#E2E8F0",
                accent_fg: "#0A0D14",
                rim: "#94A3B8",
                stroke_dark: "#05070B",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#1C2330",
                crest_trim: "#CBD5E1",
            },
            light: SkinTokens {
                bg: "#ECEFF4",
                surface: "#F7F8FA",
                surface_elevated: "#FFFFFF",
                text: "#0A0D14",
                text_muted: "#475569",
                accent: "#1E293B",
                accent_fg: "#FFFFFF",
                rim: "#334155",
                stroke_dark: "#0A0D14",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#E2E8F0",
                crest_trim: "#1E293B",
            },
        },
        UiSkinSpec {
            id: "anime-pop-dark",
            name: "Anime Pop Dark",
            mood: "top-grossing Roblox anime game UI, matte dark dot-matrix surfaces, vibrant electric-lime & azure gradient pills, thick black comic ink outlines, cel-shaded gloss",
            chroma_key: "#FF00FF",
            dark: SkinTokens {
                bg: "#0B0D14",
                surface: "#13151C",
                surface_elevated: "#1B1F2B",
                text: "#F8FAFC",
                text_muted: "#CBD5E1",
                accent: "#7CFF00",
                accent_fg: "#090B10",
                rim: "#2DD4BF",
                stroke_dark: "#05070B",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#22C55E",
                crest_trim: "#7CFF00",
            },
            light: SkinTokens {
                bg: "#E2E8F0",
                surface: "#F8FAFC",
                surface_elevated: "#FFFFFF",
                text: "#090B10",
                text_muted: "#334155",
                accent: "#65E600",
                accent_fg: "#090B10",
                rim: "#0EA5E9",
                stroke_dark: "#090B10",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#DCFCE7",
                crest_trim: "#15803D",
            },
        },
        UiSkinSpec {
            id: "meme-pop-arcade",
            name: "Meme Pop Arcade",
            mood: "chunky meme-arcade pop Roblox UI, glossy cel-shaded emerald & gold pills, crisp #000000 ink outlines, high-contrast rounded HUD plates",
            chroma_key: "#FF00FF",
            dark: SkinTokens {
                bg: "#0B0E17",
                surface: "#121520",
                surface_elevated: "#1B2030",
                text: "#F8FAFC",
                text_muted: "#CBD5E1",
                accent: "#22C55E",
                accent_fg: "#052E16",
                rim: "#FACC15",
                stroke_dark: "#000000",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#1E293B",
                crest_trim: "#38BDF8",
            },
            light: SkinTokens {
                bg: "#EDF1F7",
                surface: "#F6F8FC",
                surface_elevated: "#FFFFFF",
                text: "#0F172A",
                text_muted: "#334155",
                accent: "#16A34A",
                accent_fg: "#04120A",
                rim: "#CA8A04",
                stroke_dark: "#000000",
                danger: "#B91C1C",
                danger_fg: "#FFFFFF",
                crest_fill: "#E2E8F0",
                crest_trim: "#15803D",
            },
        },
    ]
}

pub fn find_skin(skin_id: &str) -> Result<UiSkinSpec, String> {
    let key = skin_id.trim().to_ascii_lowercase();
    visual_skins()
        .into_iter()
        .find(|s| s.id == key)
        .ok_or_else(|| format!("unknown UI Kit skin '{skin_id}'"))
}

pub fn find_layout(layout_id: &str) -> Result<LayoutTopologySpec, String> {
    let key = layout_id.trim().to_ascii_lowercase();
    layout_topologies()
        .into_iter()
        .find(|l| l.id == key)
        .ok_or_else(|| format!("unknown UI Kit layout topology '{layout_id}'"))
}

pub fn sheet_recipes() -> Vec<UiSheetRecipe> {
    vec![
        UiSheetRecipe {
            id: "chrome_sheet",
            name: "UI Chrome Sheet (9-Slice Panels, Crests, Buttons, Bars)",
            cols: 4,
            rows: 2,
            items: vec![
                SheetItemSpec {
                    name: "modal_panel",
                    row: 0,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [512, 320],
                    prompt_desc: "chamfered glass modal panel frame with crisp chromatic rim bevel",
                },
                SheetItemSpec {
                    name: "header_crest",
                    row: 0,
                    col: 1,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [440, 128],
                    prompt_desc: "original architectural kite-wing header crest plaque with central diamond inlay",
                },
                SheetItemSpec {
                    name: "btn_primary",
                    row: 0,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [320, 128],
                    prompt_desc: "glossy beveled primary action button with specular top highlight",
                },
                SheetItemSpec {
                    name: "btn_secondary",
                    row: 0,
                    col: 3,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [320, 128],
                    prompt_desc: "sleek frosted slate secondary button with crisp metallic border",
                },
                SheetItemSpec {
                    name: "btn_danger",
                    row: 1,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [320, 128],
                    prompt_desc: "glossy crimson metallic danger action button with subtle highlight",
                },
                SheetItemSpec {
                    name: "close_orb",
                    row: 1,
                    col: 1,
                    is_9slice: false,
                    cutout_interior: false,
                    target_size: [128, 128],
                    prompt_desc: "circular beveled glass close orb button with clean diagonal X glyph",
                },
                SheetItemSpec {
                    name: "progress_track",
                    row: 1,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [360, 64],
                    prompt_desc: "recessed dark metallic progress bar slot track",
                },
                SheetItemSpec {
                    name: "progress_fill",
                    row: 1,
                    col: 3,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [360, 56],
                    prompt_desc: "luminous beveled progress bar fill strip with specular gloss",
                },
            ],
        },
        UiSheetRecipe {
            id: "icons_sheet",
            name: "Game Icons Sheet (12 Cel-Shaded Vector-Crisp Icons)",
            cols: 4,
            rows: 3,
            items: vec![
                SheetItemSpec { name: "icon_summon", row: 0, col: 0, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "winged prism summon crystal" },
                SheetItemSpec { name: "icon_deck", row: 0, col: 1, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "fanned twin collector cards" },
                SheetItemSpec { name: "icon_battle", row: 0, col: 2, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "crossed ceremonial duel blades" },
                SheetItemSpec { name: "icon_codex", row: 0, col: 3, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "open scholar codex tome with bookmark" },
                SheetItemSpec { name: "icon_quest", row: 1, col: 0, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "wax-sealed mission scroll" },
                SheetItemSpec { name: "icon_shop", row: 1, col: 1, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "merchant pavilion awning emblem" },
                SheetItemSpec { name: "icon_settings", row: 1, col: 2, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "precision six-tooth mechanical gear" },
                SheetItemSpec { name: "icon_rewards", row: 1, col: 3, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "ribbon-bound gift chest" },
                SheetItemSpec { name: "icon_coin", row: 2, col: 0, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "octagonal minted gold emblem coin" },
                SheetItemSpec { name: "icon_gem", row: 2, col: 1, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "faceted luminous crystal gem" },
                SheetItemSpec { name: "icon_spark", row: 2, col: 2, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "stylized energy bolt emblem" },
                SheetItemSpec { name: "icon_shield", row: 2, col: 3, is_9slice: false, cutout_interior: true, target_size: [256, 256], prompt_desc: "beveled kite guard shield" },
            ],
        },
        UiSheetRecipe {
            id: "rarity_frames_sheet",
            name: "Rarity Card Frames Sheet (Common -> Mythic + Legacy)",
            cols: 4,
            rows: 2,
            items: vec![
                SheetItemSpec { name: "frame_common", row: 0, col: 0, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Common brushed slate portrait card frame with hollow center" },
                SheetItemSpec { name: "frame_uncommon", row: 0, col: 1, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Uncommon verdigris emerald beveled portrait card frame with hollow center" },
                SheetItemSpec { name: "frame_rare", row: 0, col: 2, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Rare cobalt prism double-border portrait card frame with hollow center" },
                SheetItemSpec { name: "frame_epic", row: 0, col: 3, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Epic amethyst chamfered corner-bracket card frame with hollow center" },
                SheetItemSpec { name: "frame_legendary", row: 1, col: 0, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Legendary gilded solar architrave card frame with hollow center" },
                SheetItemSpec { name: "frame_mythic", row: 1, col: 1, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Mythic iridescent cyan-magenta winged crest card frame with hollow center" },
                SheetItemSpec { name: "frame_legacy", row: 1, col: 2, is_9slice: true, cutout_interior: true, target_size: [512, 712], prompt_desc: "Legacy ivory-and-gold filigree heirloom card frame with hollow center" },
            ],
        },
        UiSheetRecipe {
            id: "card_borders_sheet",
            name: "Sleek Edge-Hugging Collector Card Borders Sheet (Common -> Mythic + Legacy)",
            cols: 4,
            rows: 2,
            items: vec![
                SheetItemSpec {
                    name: "border_common",
                    row: 0,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Common brushed pewter edge-hugging collector card border with crisp chamfered corner notches, side border thickness <=6%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_uncommon",
                    row: 0,
                    col: 1,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Uncommon emerald double-pinline edge-hugging collector card border with stepped corner notches, side border thickness <=6%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_rare",
                    row: 0,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Rare sapphire edge-hugging collector card border with twin-blade upper wing fins and lower corner chevrons, side border thickness <=6%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_epic",
                    row: 0,
                    col: 3,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Epic violet sigil corner-bracket edge-hugging collector card border with geometric inlaid corner wards, side border thickness <=7%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_legendary",
                    row: 1,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Legendary amber temple corner-filigree edge-hugging collector card border with slim top architrave keystone, side border thickness <=7%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_mythic",
                    row: 1,
                    col: 1,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Mythic iridescent cyan-magenta celestial prism top crest and corner energy wisps edge-hugging collector card border, side border thickness <=8%, >=88% open hollow chroma-key window",
                },
                SheetItemSpec {
                    name: "border_legacy",
                    row: 1,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: true,
                    target_size: [512, 712],
                    prompt_desc: "sleek ultra-thin Legacy alabaster-gold triumphal corner crown edge-hugging collector card border with heirloom clasp, side border thickness <=8%, >=88% open hollow chroma-key window",
                },
            ],
        },
        UiSheetRecipe {
            id: "panels_sheet",
            name: "Image-Gen Panels Sheet (Modal Glass, Title Ribbon, Tabs, CTA Buttons, Dock Tile, Inspector Card)",
            cols: 4,
            rows: 2,
            items: vec![
                SheetItemSpec {
                    name: "modal_glass",
                    row: 0,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [512, 320],
                    prompt_desc: "sleek jet-black obsidian translucent modal panel with thin vibrant neon-gradient outer rim glow and crisp minimal bevels",
                },
                SheetItemSpec {
                    name: "title_ribbon",
                    row: 0,
                    col: 1,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [512, 128],
                    prompt_desc: "dramatic breaking-out diagonal angular slash-crest header banner with a radiant geometric flare accent on the left, fully original geometry with zero trademarked emblems",
                },
                SheetItemSpec {
                    name: "tab_active",
                    row: 0,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [256, 96],
                    prompt_desc: "single brushed metallic gold beveled filter tab plate with specular sheen",
                },
                SheetItemSpec {
                    name: "tab_inactive",
                    row: 0,
                    col: 3,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [256, 96],
                    prompt_desc: "single brushed dark titanium steel beveled filter tab plate",
                },
                SheetItemSpec {
                    name: "btn_emerald",
                    row: 1,
                    col: 0,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [320, 112],
                    prompt_desc: "glossy emerald-green beveled primary call-to-action button with top specular sheen",
                },
                SheetItemSpec {
                    name: "btn_gold",
                    row: 1,
                    col: 1,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [320, 112],
                    prompt_desc: "glossy amber-gold metallic call-to-action button with top specular sheen",
                },
                SheetItemSpec {
                    name: "dock_tile",
                    row: 1,
                    col: 2,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [160, 160],
                    prompt_desc: "glossy dark obsidian square hotkey dock tile with diagonal glass reflection and crisp rim",
                },
                SheetItemSpec {
                    name: "inspector_card",
                    row: 1,
                    col: 3,
                    is_9slice: true,
                    cutout_interior: false,
                    target_size: [440, 320],
                    prompt_desc: "sleek dark translucent detail inspector sub-panel frame with glowing accent edge",
                },
            ],
        },
        UiSheetRecipe {
            id: "cartoon_icons_sheet",
            name: "Pop-Anime Cartoon Icon Pack (12 Chunky Outlined Icons)",
            cols: 4,
            rows: 3,
            items: vec![
                SheetItemSpec { name: "icon_backpack", row: 0, col: 0, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "chunky tan and brown leather adventure backpack with brass buckles and glossy highlight, thick black comic ink outline" },
                SheetItemSpec { name: "icon_summon", row: 0, col: 1, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "radiant violet-cyan star portal crystal with swirling energy spark, thick black comic ink outline" },
                SheetItemSpec { name: "icon_swords", row: 0, col: 2, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "crossed silver and azure anime broadswords with golden hilts and impact flash, thick black comic ink outline" },
                SheetItemSpec { name: "icon_shop", row: 0, col: 3, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "vibrant emerald and gold potion shop pouch with glossy ribbon tie, thick black comic ink outline" },
                SheetItemSpec { name: "icon_coin", row: 1, col: 0, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "chunky bright green-emerald cash bill stack and shiny gold emblem, thick black comic ink outline" },
                SheetItemSpec { name: "icon_gem", row: 1, col: 1, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "vivid magenta-violet faceted diamond crystal gem with crisp white specular facet, thick black comic ink outline" },
                SheetItemSpec { name: "icon_clover", row: 1, col: 2, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "bright neon lime-green glossy four-leaf luck clover with gold sparkle dots, thick black comic ink outline" },
                SheetItemSpec { name: "icon_potion", row: 1, col: 3, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "glowing sky-blue wing-emblem speed potion bottle with cork stopper and bubbles, thick black comic ink outline" },
                SheetItemSpec { name: "icon_crown", row: 2, col: 0, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "chunky bright gold royal crown with sapphire center jewel, thick black comic ink outline" },
                SheetItemSpec { name: "icon_scroll", row: 2, col: 1, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "warm parchment quest scroll bound with crimson wax seal and gold rod caps, thick black comic ink outline" },
                SheetItemSpec { name: "icon_portal", row: 2, col: 2, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "electric cyan swirling dungeon vortex ring over dark stone base, thick black comic ink outline" },
                SheetItemSpec { name: "icon_gear", row: 2, col: 3, is_9slice: false, cutout_interior: false, target_size: [256, 256], prompt_desc: "sleek cool silver steel settings cogwheel with azure center hub, thick black comic ink outline" },
            ],
        },
    ]
}

pub fn find_sheet_recipe(sheet_id: &str) -> Result<UiSheetRecipe, String> {
    let key = sheet_id.trim().to_ascii_lowercase();
    sheet_recipes()
        .into_iter()
        .find(|r| r.id == key)
        .ok_or_else(|| format!("unknown UI Kit sheet recipe '{sheet_id}'"))
}

pub fn slice_items_for_recipe(sheet_id: &str) -> Result<Vec<Value>, String> {
    let recipe = find_sheet_recipe(sheet_id)?;
    let is_borders = recipe.id == "card_borders_sheet";
    Ok(recipe
        .items
        .into_iter()
        .map(|it| {
            let mut obj = json!({
                "name": it.name,
                "row": it.row,
                "col": it.col,
                "rows": recipe.rows,
                "cols": recipe.cols,
                "is_9slice": it.is_9slice,
                "cutout_interior": it.cutout_interior,
                "target_size": it.target_size,
            });
            if is_borders {
                obj["pad_px"] = json!(2);
            }
            obj
        })
        .collect())
}

/// Build a single-line, zero-text, zero-franchise-likeness ChatGPT Image Gen prompt
/// on a flat uniform chroma-key background for clean alpha cutout and auto-slicing.
pub fn build_ui_sheet_prompt(skin_id: &str, sheet_id: &str) -> Result<String, String> {
    let skin = find_skin(skin_id)?;
    let recipe = find_sheet_recipe(sheet_id)?;
    let items_list: Vec<&str> = recipe.items.iter().map(|i| i.prompt_desc).collect();
    let joined = items_list.join("; ");
    let chroma = if recipe.id == "card_borders_sheet" {
        "#00FF00"
    } else {
        skin.chroma_key
    };
    let style_clause = if skin.id == "anime-pop-dark" || recipe.id == "cartoon_icons_sheet" {
        "clean modern top-grossing Roblox game UI flat rounded matte dark dot-matrix panels (#13151C) and vibrant glossy cel-shaded vertical gradient bars and chunky icons with thick crisp #000000 comic ink outlines, ZERO medieval bevel shields, ZERO metal spikes, ZERO gothic stone frames"
    } else {
        "razor-sharp crisp vector-painted bevels and cel-shaded highlights"
    };
    let raw = format!(
        "2D game UI sprite sheet in {} aesthetic ({}), cleanly arranged in an evenly spaced {}x{} grid with generous gutters on a flat uniform {} chroma-key background for clean alpha cutout, {}, containing: {}, strictly zero text, no letters, no numbers, no watermarks, no logos, original geometric shapes with zero anime franchise likenesses and zero crimson star ribbons",
        skin.name,
        skin.mood,
        recipe.cols,
        recipe.rows,
        chroma,
        style_clause,
        joined
    );
    Ok(raw
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

/// Build a single-line prompt for reference-screenshot-guided UI kit variants (`--mode edit --ref-image ...`).
/// When `ref_count > 0`, instructs the image generator to study the attached screenshot(s) for
/// layout hierarchy, stroke weight, outline, and gloss while producing a tweaked, distinct original
/// variant (NOT a 1:1 clone) in the target theme/palette on a uniform chroma-key background.
pub fn build_ui_variant_prompt(
    skin_id: &str,
    sheet_id: &str,
    variant_theme: Option<&str>,
    ref_count: usize,
) -> Result<String, String> {
    let skin = find_skin(skin_id)?;
    let recipe = find_sheet_recipe(sheet_id)?;
    let theme_clean = variant_theme
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if ref_count == 0 && theme_clean.is_none() {
        return build_ui_sheet_prompt(skin.id, recipe.id);
    }
    let items_list: Vec<&str> = recipe.items.iter().map(|i| i.prompt_desc).collect();
    let joined = items_list.join("; ");
    let chroma = if recipe.id == "card_borders_sheet" {
        "#00FF00"
    } else {
        skin.chroma_key
    };
    let theme_desc = theme_clean.unwrap_or(skin.name);
    let palette_desc = format!(
        "surface {}, accent {}, rim {}, outline {}",
        skin.dark.surface, skin.dark.accent, skin.dark.rim, skin.dark.stroke_dark
    );
    let ref_clause = if ref_count > 0 {
        format!(
            "Using the {ref_count} attached reference screenshot(s) strictly as a layout hierarchy, stroke weight, outline, and gloss guide, produce a tweaked, distinct original variant (NOT a 1:1 clone) in '{theme_desc}' theme ({palette_desc})"
        )
    } else {
        format!("In '{theme_desc}' theme ({palette_desc})")
    };
    let raw = format!(
        "{ref_clause}, formatted as a 2D game UI sprite sheet in {} aesthetic ({}), evenly spaced {}x{} grid with generous gutters on a flat uniform {} chroma-key background for clean alpha cutout, containing: {}, strictly zero text, no letters, no numbers, no watermarks, and no copied logos",
        skin.name,
        skin.mood,
        recipe.cols,
        recipe.rows,
        chroma,
        joined
    );
    Ok(raw.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn srgb_channel_to_linear(c: u8) -> f64 {
    let v = f64::from(c) / 255.0;
    if v <= 0.03928 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn parse_hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let clean = hex.trim().trim_start_matches('#');
    if clean.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&clean[0..2], 16).ok()?;
    let g = u8::from_str_radix(&clean[2..4], 16).ok()?;
    let b = u8::from_str_radix(&clean[4..6], 16).ok()?;
    Some([r, g, b])
}

/// Compute WCAG 2.1 contrast ratio (1.0 .. 21.0) between two `#RRGGBB` hex colors.
pub fn wcag_contrast_ratio(fg_hex: &str, bg_hex: &str) -> f64 {
    let Some([fr, fg, fb]) = parse_hex_rgb(fg_hex) else {
        return 1.0;
    };
    let Some([br, bg, bb]) = parse_hex_rgb(bg_hex) else {
        return 1.0;
    };
    let lum = |r: u8, g: u8, b: u8| -> f64 {
        0.2126 * srgb_channel_to_linear(r)
            + 0.7152 * srgb_channel_to_linear(g)
            + 0.0722 * srgb_channel_to_linear(b)
    };
    let l1 = lum(fr, fg, fb);
    let l2 = lum(br, bg, bb);
    let (hi, lo) = if l1 >= l2 { (l1, l2) } else { (l2, l1) };
    (hi + 0.05) / (lo + 0.05)
}

pub fn presets_json() -> Value {
    let rules: Vec<Value> = ui_craft_rules()
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "number": r.number,
                "title": r.title,
                "rule": r.rule,
                "roblox_binding": r.roblox_binding,
                "anti_pattern": r.anti_pattern,
            })
        })
        .collect();
    let layouts: Vec<Value> = layout_topologies()
        .into_iter()
        .map(|l| {
            json!({
                "id": l.id,
                "name": l.name,
                "summary": l.summary,
                "anchor_zones": l.anchor_zones,
                "mobile_safe": l.mobile_safe,
            })
        })
        .collect();
    let skins: Vec<Value> = visual_skins()
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "name": s.name,
                "mood": s.mood,
                "chroma_key": s.chroma_key,
                "dark": s.dark.to_json(),
                "light": s.light.to_json(),
            })
        })
        .collect();
    let sheets: Vec<Value> = sheet_recipes()
        .into_iter()
        .map(|r| {
            json!({
                "id": r.id,
                "name": r.name,
                "cols": r.cols,
                "rows": r.rows,
                "item_count": r.items.len(),
                "items": r.items.iter().map(|i| json!({
                    "name": i.name,
                    "is_9slice": i.is_9slice,
                    "cutout_interior": i.cutout_interior,
                    "target_size": i.target_size,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "ok": true,
        "craft_rules": rules,
        "layouts": layouts,
        "skins": skins,
        "sheets": sheets,
        "external_adapters": [
            "chatgpt-browser-cli",
            "openai-images-http",
            "recraft-v3-ui",
            "layer-ai-ui",
            "scenario-game-ui",
        ],
    })
}

pub fn prompt_guidance() -> &'static str {
    "UI Craft (10 Rules, zero copying): ui_kit_presets/ui_kit_generate -> 9-slice transparent PNGs + AssetIds.slices (chrome_sheet, panels_sheet, icons_sheet, cartoon_icons_sheet, card_borders_sheet -> card-borders/*), BlurEffect modal depth, >=5.25:1 Dark/Light glass, >=44px CTA + UIStroke numerals, Master-Detail + 844x390 safe; render full-bleed edge-to-edge ImageLabel portraits (UDim2.fromScale(1, 1), Enum.ScaleType.Crop) overlaid with sleek card-borders/* + Anime Nexus RNG X perimeter icon docks + bottom hotbar, using skins 'anime-pop-dark' or 'meme-pop-arcade' matte dark panels, vibrant vertical UIGradient headers/pills, chunky #000000-outlined cartoon_icons_sheet icons, frameless glowing HUD counters/pity bars, and Blender 3D maps via blender_build_map (NEVER heavy medieval metallic shields or dated medieval bevel chrome). Image-Gen-First & Reference Variants: when a live game teardown has reference screenshots, ui_kit_generate defaults use_teardown_reference=true to pass up to 3 screenshots into ChatGPT (--mode edit --ref-image ...) as layout hierarchy/weight/outline/gloss guides while producing a tweaked original variant (variant_theme, NEVER a 1:1 clone) with character_art=false; generate cartoon_icons_sheet, panels_sheet, chrome_sheet, and card_borders_sheet via ui_kit_generate {\"provider\":\"chatgpt-browser-cli\",\"procedural_fallback\":false}; bind >=70% to sliced PNGs with BackgroundTransparency = 1 on slice-backed parents; verify via studio_verify_ui_craft."
}

fn collect_explicit_ui_refs(body: &Value) -> Vec<String> {
    let mut out = Vec::new();
    let mut push_unique = |raw: &str| {
        let t = raw.trim();
        if !t.is_empty() && !out.iter().any(|p| p == t) && out.len() < 8 {
            out.push(t.to_string());
        }
    };
    for key in ["reference_screenshots", "reference_images", "ref_images", "references"] {
        if let Some(arr) = body.get(key).and_then(Value::as_array) {
            for v in arr {
                if let Some(s) = v.as_str() {
                    push_unique(s);
                }
            }
        }
    }
    for key in ["reference_screenshot", "reference_image", "ref_image"] {
        if let Some(s) = body.get(key).and_then(Value::as_str) {
            push_unique(s);
        }
    }
    out
}

/// Generate (or slice an existing) UI Kit asset sheet + build the matching mixable ScreenDoc.
pub fn generate_ui_kit(
    root: &Path,
    store: &SecretStore,
    project_id: Option<&str>,
    body: &Value,
) -> Result<Value, Oc> {
    let skin_id = body
        .get("skin")
        .and_then(Value::as_str)
        .unwrap_or("obsidian-prism");
    let skin = find_skin(skin_id).map_err(|msg| Oc { status: 400, msg })?;

    let sheet_id = body
        .get("sheet")
        .and_then(Value::as_str)
        .unwrap_or("chrome_sheet");
    let _recipe = find_sheet_recipe(sheet_id).map_err(|msg| Oc { status: 400, msg })?;

    let layout_id = body
        .get("layout")
        .and_then(Value::as_str)
        .unwrap_or("skybridge-crest");
    let _layout = find_layout(layout_id).map_err(|msg| Oc { status: 400, msg })?;

    let theme_mode = body
        .get("theme_mode")
        .and_then(Value::as_str)
        .unwrap_or("dark");

    let use_teardown_reference = body
        .get("use_teardown_reference")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let reference_teardown_id = body
        .get("reference_teardown_id")
        .and_then(Value::as_u64);
    let variant_theme = body
        .get("variant_theme")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let explicit_refs = collect_explicit_ui_refs(body);
    let reference_images = if !explicit_refs.is_empty() {
        explicit_refs
    } else if use_teardown_reference {
        crate::teardown::default_teardown_ui_references(root, reference_teardown_id)
    } else {
        Vec::new()
    };
    let reference_guided = !reference_images.is_empty();
    let variant_mode = if reference_guided {
        "reference_variant"
    } else {
        "clean_sheet"
    };

    let prompt = build_ui_variant_prompt(skin.id, sheet_id, variant_theme, reference_images.len())
        .map_err(|msg| Oc { status: 400, msg })?;
    let default_sheet_rel = format!("art/ui/sheets/{}_{}.png", skin.id, sheet_id);

    let procedural_fallback = body
        .get("procedural_fallback")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || body
            .get("provider")
            .and_then(Value::as_str)
            .is_some_and(|p| p.eq_ignore_ascii_case("procedural"));

    let (sheet_rel, gen_meta) = if let Some(existing) = body
        .get("sheet_path")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        (existing.to_string(), json!({ "source": "sheet_path", "path": existing }))
    } else if procedural_fallback {
        let pal = if theme_mode.eq_ignore_ascii_case("light") {
            skin.light.to_json()
        } else {
            skin.dark.to_json()
        };
        let chroma = if sheet_id.eq_ignore_ascii_case("card_borders_sheet") {
            "#00FF00"
        } else {
            skin.chroma_key
        };
        match render_procedural_ui_sheet(
            root,
            sheet_id,
            &default_sheet_rel,
            &pal,
            chroma,
        ) {
            Ok(rendered) => (
                default_sheet_rel.clone(),
                json!({ "source": "procedural_fallback", "master": rendered }),
            ),
            Err(oc) if oc.status == 500 || oc.status == 503 => {
                // Zero-dependency pure-Rust RGBA PNG fallback when Pillow is absent (e.g. minimal CI sandbox).
                let mut res = generate_rust_fallback_kit(
                    root,
                    &skin,
                    sheet_id,
                    layout_id,
                    theme_mode,
                    &prompt,
                    project_id,
                    body,
                )?;
                if let Some(obj) = res.as_object_mut() {
                    obj.insert("reference_guided".into(), json!(reference_guided));
                    obj.insert("reference_images".into(), json!(reference_images));
                    obj.insert("variant_mode".into(), json!(variant_mode));
                    obj.insert("variant_theme".into(), json!(variant_theme));
                }
                return Ok(res);
            }
            Err(oc) => return Err(oc),
        }
    } else {
        let raw_provider = body
            .get("provider")
            .or_else(|| body.get("adapter_id"))
            .and_then(Value::as_str)
            .unwrap_or("chatgpt-browser-cli");
        let mut gen_body = json!({
            "project": project_id.unwrap_or(""),
            "provider": raw_provider,
            "kind": "ui_sheet",
            "character_art": false,
            "prompt": prompt,
            "out_path": default_sheet_rel,
            "transparent": raw_provider.contains("openai"),
        });
        if !reference_images.is_empty() {
            gen_body["reference_images"] = json!(reference_images);
        }

        let data_dir = body
            .get("data_dir")
            .and_then(Value::as_str)
            .map(Path::new)
            .unwrap_or(root);
        let explicit_adapter = body.get("adapter_id").and_then(Value::as_str).or_else(|| {
            let low = raw_provider.to_ascii_lowercase();
            if low == "chatgpt-browser-cli" || low == "chatgpt" {
                None
            } else {
                Some(raw_provider)
            }
        });
        let actx = InvokeCtx {
            data_dir,
            assets_root: data_dir,
            store,
            project_dir: Some(root),
        };
        let gen_res = if let Some(res) = maybe_invoke_override(&actx, "image_gen", explicit_adapter, &gen_body) {
            res.map_err(|msg| Oc { status: 502, msg })?
        } else {
            let mut b = gen_body.clone();
            if raw_provider.eq_ignore_ascii_case("chatgpt-browser-cli") {
                b["provider"] = json!("chatgpt");
            }
            art_generate(store, root, &b)?
        };
        if gen_res.get("ok").and_then(Value::as_bool) != Some(true) {
            return Ok(json!({
                "ok": false,
                "status": gen_res.get("status").cloned().unwrap_or_else(|| json!("failed")),
                "error": gen_res.get("error").cloned().unwrap_or_else(|| json!("UI sheet generation failed")),
                "generation": gen_res,
                "skin": skin.id,
                "sheet": sheet_id,
                "layout": layout_id,
                "prompt": prompt,
                "reference_guided": reference_guided,
                "reference_images": reference_images,
                "variant_mode": variant_mode,
                "variant_theme": variant_theme,
            }));
        }
        let path = gen_res
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or(&default_sheet_rel)
            .to_string();
        (path, json!({ "source": raw_provider, "generation": gen_res }))
    };

    let mut slice_body = json!({
        "project": project_id.unwrap_or(""),
        "input": sheet_rel,
        "skin": skin.id,
        "sheet": sheet_id,
    });
    if let Some(out_dir) = body.get("out_dir").and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()) {
        slice_body["out_dir"] = json!(out_dir);
    }
    let sliced = slice_ui_sheet(root, &slice_body)?;
    let screen_doc = ui_kit_screen_doc(
        layout_id,
        skin.id,
        Some(theme_mode),
        body.get("slug").and_then(Value::as_str),
    )
    .map_err(|msg| Oc { status: 400, msg })?;

    let saved_screen = if body.get("save_screen").and_then(Value::as_bool) == Some(true) {
        let slug = body
            .get("slug")
            .and_then(Value::as_str)
            .unwrap_or(layout_id);
        let save_res = save_project_screen(root, slug, &json!({ "doc": screen_doc.to_json() }))
            .map_err(|msg| Oc { status: 400, msg })?;
        Some(save_res)
    } else {
        None
    };

    Ok(json!({
        "ok": true,
        "skin": skin.id,
        "sheet": sheet_id,
        "layout": layout_id,
        "theme_mode": theme_mode,
        "prompt": prompt,
        "sheet_path": sheet_rel,
        "reference_guided": reference_guided,
        "reference_images": reference_images,
        "variant_mode": variant_mode,
        "variant_theme": variant_theme,
        "generation": gen_meta,
        "slices_json": sliced.get("slices_json").cloned().unwrap_or(Value::Null),
        "sliced_count": sliced.get("sliced_count").cloned().unwrap_or(json!(0)),
        "nine_slice_count": sliced.get("nine_slice_count").cloned().unwrap_or(json!(0)),
        "sliced": sliced.get("sliced").cloned().unwrap_or_else(|| json!([])),
        "screen_doc": screen_doc.to_json(),
        "saved_screen": saved_screen,
    }))
}

fn png_crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn png_chunk(out: &mut Vec<u8>, tag: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(body);
    let mut crc_buf = Vec::with_capacity(4 + body.len());
    crc_buf.extend_from_slice(tag);
    crc_buf.extend_from_slice(body);
    out.extend_from_slice(&png_crc32(&crc_buf).to_be_bytes());
}

fn encode_rgba_png(w: u32, h: u32, rgba: &[u8]) -> Result<Vec<u8>, Oc> {
    use flate2::{write::ZlibEncoder, Compression};
    use std::io::Write;
    let mut raw = Vec::with_capacity((w as usize * 4 + 1) * h as usize);
    for y in 0..h as usize {
        raw.push(0);
        let row_start = y * w as usize * 4;
        raw.extend_from_slice(&rgba[row_start..row_start + w as usize * 4]);
    }
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&raw)
        .and_then(|_| enc.finish())
        .map_err(|e| Oc { status: 500, msg: format!("compress png: {e}") })
        .map(|idat| {
            let mut png = Vec::with_capacity(64 + idat.len());
            png.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
            let mut ihdr = [0u8; 13];
            ihdr[0..4].copy_from_slice(&w.to_be_bytes());
            ihdr[4..8].copy_from_slice(&h.to_be_bytes());
            ihdr[8] = 8;
            ihdr[9] = 6; // RGBA
            png_chunk(&mut png, b"IHDR", &ihdr);
            png_chunk(&mut png, b"IDAT", &idat);
            png_chunk(&mut png, b"IEND", &[]);
            png
        })
}

fn generate_rust_fallback_kit(
    root: &Path,
    skin: &UiSkinSpec,
    sheet_id: &str,
    layout_id: &str,
    theme_mode: &str,
    prompt: &str,
    project_id: Option<&str>,
    body: &Value,
) -> Result<Value, Oc> {
    let recipe = find_sheet_recipe(sheet_id).map_err(|msg| Oc { status: 400, msg })?;
    let tok = if theme_mode.eq_ignore_ascii_case("light") {
        &skin.light
    } else {
        &skin.dark
    };
    let fill = parse_hex_rgb(tok.surface_elevated).unwrap_or([20, 25, 38]);
    let rim = parse_hex_rgb(tok.rim).unwrap_or([56, 189, 248]);
    let accent = parse_hex_rgb(tok.accent).unwrap_or([56, 189, 248]);

    let mut out_items = Vec::with_capacity(recipe.items.len());
    let mut nine_slice_count = 0usize;
    let slices_rel = "art/ui/slices.json";

    for (idx, item) in recipe.items.iter().enumerate() {
        let w = item.target_size[0].min(256);
        let h = item.target_size[1].min(256);
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        let pad = 6u32;
        let rim_w = 4u32;
        let base_col = if item.name.contains("primary") || item.name.contains("fill") || idx % 3 == 0 {
            accent
        } else {
            fill
        };
        for y in pad..(h - pad) {
            for x in pad..(w - pad) {
                let in_hollow = item.cutout_interior
                    && x >= pad + 18
                    && x < w - pad - 18
                    && y >= pad + 18
                    && y < h - pad - 18;
                if in_hollow {
                    continue;
                }
                let is_border = x < pad + rim_w
                    || x >= w - pad - rim_w
                    || y < pad + rim_w
                    || y >= h - pad - rim_w;
                let rgb = if is_border { rim } else { base_col };
                let off = ((y * w + x) * 4) as usize;
                rgba[off] = rgb[0];
                rgba[off + 1] = rgb[1];
                rgba[off + 2] = rgb[2];
                rgba[off + 3] = 255;
            }
        }
        let png_bytes = encode_rgba_png(w, h, &rgba)?;
        let rel = if sheet_id.eq_ignore_ascii_case("card_borders_sheet") {
            format!("art/ui/card-borders/{}.png", item.name)
        } else {
            format!("art/ui/{}/{}.png", skin.id, item.name)
        };
        let abs = safe_rel(root, &rel)?;
        if let Some(p) = abs.parent() {
            std::fs::create_dir_all(p).map_err(|e| Oc { status: 500, msg: format!("mkdir: {e}") })?;
        }
        std::fs::write(&abs, &png_bytes).map_err(|e| Oc { status: 500, msg: format!("write {rel}: {e}") })?;
        let slice = if item.is_9slice {
            let sl = [
                (pad + rim_w + 8) as i64,
                (pad + rim_w + 8) as i64,
                (w - pad - rim_w - 8) as i64,
                (h - pad - rim_w - 8) as i64,
            ];
            update_slices_json(root, slices_rel, &rel, &sl, u64::from(w), u64::from(h))?;
            nine_slice_count += 1;
            json!(sl)
        } else {
            Value::Null
        };
        let sha = sha256_hex(&png_bytes);
        let _ = append_art_log(
            root,
            &json!({
                "at": now_iso(),
                "action": "slice_ui_sheet",
                "skin": skin.id,
                "path": rel,
                "width": w,
                "height": h,
                "bytes": png_bytes.len(),
                "sha256": sha,
                "has_alpha": true,
                "slice": slice,
            }),
        );
        out_items.push(json!({
            "name": item.name,
            "path": rel,
            "width": w,
            "height": h,
            "bytes": png_bytes.len(),
            "sha256": sha,
            "has_alpha": true,
            "is_9slice": item.is_9slice,
            "slice": slice,
        }));
    }

    let screen_doc = ui_kit_screen_doc(
        layout_id,
        skin.id,
        Some(theme_mode),
        body.get("slug").and_then(Value::as_str),
    )
    .map_err(|msg| Oc { status: 400, msg })?;
    let saved_screen = if body.get("save_screen").and_then(Value::as_bool) == Some(true) {
        let slug = body.get("slug").and_then(Value::as_str).unwrap_or(layout_id);
        Some(
            save_project_screen(root, slug, &json!({ "doc": screen_doc.to_json() }))
                .map_err(|msg| Oc { status: 400, msg })?,
        )
    } else {
        None
    };

    Ok(json!({
        "ok": true,
        "skin": skin.id,
        "sheet": sheet_id,
        "layout": layout_id,
        "theme_mode": theme_mode,
        "prompt": prompt,
        "sheet_path": format!("art/ui/sheets/{}_{}.png", skin.id, sheet_id),
        "generation": { "source": "procedural_fallback_rust", "project_id": project_id },
        "slices_json": if nine_slice_count > 0 { json!(slices_rel) } else { Value::Null },
        "sliced_count": out_items.len(),
        "nine_slice_count": nine_slice_count,
        "sliced": out_items,
        "screen_doc": screen_doc.to_json(),
        "saved_screen": saved_screen,
    }))
}
