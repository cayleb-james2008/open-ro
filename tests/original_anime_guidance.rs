//! Regression guard enforcing 100% original anime-style character guidance
//! across OpenRO's art knowledge, library packs, ideas, market taste, frontend,
//! web-engine, and card-battler Luau templates.

use openro_rs::art_knowledge;
use std::path::Path;

const TARGET_FILES: &[&str] = &[
    "src/art_knowledge.rs",
    "src/library.rs",
    "src/ideas.rs",
    "src/market_taste.rs",
    "src/card_gallery_world.rn",
    "src/card_gacha_roller.rn",
    "src/tower_progression.rn",
    "frontend/src/app/marketTaste.ts",
    "frontend/src/app/data.ts",
    "frontend/web-engine/luau-skills.ts",
    "frontend/web-engine/install.ts",
];

const FORBIDDEN_PHRASES: &[&str] = &[
    "unmistakably recognizable",
    "anime-homage",
    "fan-art",
    "joyful pirate",
    "six-eyes sorcerer",
    "shadow monarch",
    "nine-tail shinobi",
    "thunder flash blade",
    "steel alchemist",
    "gravity ronin",
    "golden ascended warrior",
];

#[test]
fn repo_contains_zero_fan_art_homage_or_lookalike_epithet_guidance() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut violations = Vec::new();

    for rel in TARGET_FILES {
        let path = manifest_dir.join(rel);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
        let lower = text.to_ascii_lowercase();
        for phrase in FORBIDDEN_PHRASES {
            if lower.contains(phrase) {
                violations.push(format!("{rel} contains forbidden phrase {phrase:?}"));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "found forbidden fan-art/homage/look-alike references:\n{}",
        violations.join("\n")
    );
}

#[test]
fn art_knowledge_enforces_original_anime_style_character_design() {
    let visual_rule = art_knowledge::VISUAL_FIDELITY_RULE.to_ascii_lowercase();
    assert!(
        visual_rule.contains("original")
            && visual_rule.contains("silhouette")
            && visual_rule.contains("costume")
            && visual_rule.contains("lore")
            && visual_rule.contains("zero copied"),
        "VISUAL_FIDELITY_RULE must enforce original silhouettes, costumes, lore, and zero copied likenesses: {}",
        art_knowledge::VISUAL_FIDELITY_RULE
    );

    let naming_rule = art_knowledge::TITLE_EPITHET_NAMING_RULE.to_ascii_lowercase();
    assert!(
        naming_rule.contains("original")
            && (naming_rule.contains("zero") || naming_rule.contains("never")),
        "TITLE_EPITHET_NAMING_RULE must enforce original names: {}",
        art_knowledge::TITLE_EPITHET_NAMING_RULE
    );

    let guidance = art_knowledge::prompt_guidance();
    let guidance_low = guidance.to_ascii_lowercase();
    assert!(
        guidance_low.contains("100% original")
            && guidance_low.contains("original silhouettes")
            && guidance_low.contains("costumes")
            && guidance_low.contains("lore")
            && guidance_low.contains("names")
            && guidance_low.contains("zero copied"),
        "prompt_guidance() must explicitly enforce 100% original anime-style character design: {guidance}"
    );
    assert!(!guidance_low.contains("rune"));

    let summary = art_knowledge::summary_json();
    let summary_str = summary.to_string().to_ascii_lowercase();
    for phrase in FORBIDDEN_PHRASES {
        assert!(
            !summary_str.contains(phrase),
            "summary_json() leaked forbidden phrase {phrase:?}"
        );
    }
    assert!(!summary_str.contains("rune"));

    let archetypes = art_knowledge::anime_archetypes();
    assert_eq!(archetypes.len(), 8);
    for a in archetypes {
        let prompt_low = a.art_prompt.to_ascii_lowercase();
        assert!(
            prompt_low.contains("eye-level") && prompt_low.contains("three-quarter"),
            "archetype {} missing eye-level neutral three-quarter framing: {}",
            a.id,
            a.art_prompt
        );
        assert!(
            !prompt_low.contains("dynamic action angle"),
            "archetype {} violates angle gate",
            a.id
        );
        for phrase in FORBIDDEN_PHRASES {
            assert!(
                !a.homage_title.to_ascii_lowercase().contains(phrase)
                    && !a.epithet_title.to_ascii_lowercase().contains(phrase)
                    && !prompt_low.contains(phrase),
                "archetype {} leaked forbidden phrase {phrase:?}",
                a.id
            );
        }
    }

    let built = art_knowledge::build_homage_prompt(
        "Helios Clockwork Paragon",
        "Flame",
        "Secret",
        "porcelain-white greaves and orbital sundial halo",
    );
    let built_low = built.to_ascii_lowercase();
    assert!(
        built_low.contains("original")
            && built_low.contains("eye-level")
            && built_low.contains("zero copied")
            && !built.contains('\n'),
        "build_homage_prompt() must enforce original anime-style character design on a single line: {built}"
    );
}
