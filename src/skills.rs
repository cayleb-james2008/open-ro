//! Rune-backed code-generation skills.
//!
//! The Luau the agent applies is genuinely produced by executing embedded
//! Rune scripts (`.rn`) on a Rune VM — there is no Rust-side string template
//! for game code.

use std::sync::Arc;

const ROUND_MANAGER_RN: &str = include_str!("round_manager.rn");

/// Run the `round_manager` Rune skill and return the generated Luau source.
pub fn generate_round_manager(create_round: bool) -> Result<String, String> {
    run_skill("generate_round_manager", create_round)
}

fn run_skill(function: &str, arg: bool) -> Result<String, String> {
    let context = rune::Context::with_default_modules()
        .map_err(|e| format!("rune context: {e}"))?;
    let runtime = Arc::new(
        context
            .runtime()
            .map_err(|e| format!("rune runtime: {e}"))?,
    );

    let mut sources = rune::Sources::new();
    sources
        .insert(
            rune::Source::memory(ROUND_MANAGER_RN)
                .map_err(|e| format!("rune source: {e}"))?,
        )
        .map_err(|e| format!("rune sources: {e}"))?;

    let mut diagnostics = rune::Diagnostics::new();
    let result = rune::prepare(&mut sources)
        .with_context(&context)
        .with_diagnostics(&mut diagnostics)
        .build();

    if !diagnostics.is_empty() {
        return Err(format!("rune diagnostics: {diagnostics:?}"));
    }
    let unit = result.map_err(|e| format!("rune build: {e}"))?;

    let mut vm = rune::Vm::new(runtime, Arc::new(unit));
    let output = vm
        .call([function], (arg,))
        .map_err(|e| format!("rune call {function}: {e}"))?;
    let text: String =
        rune::from_value(output).map_err(|e| format!("rune return value: {e}"))?;
    Ok(text)
}
