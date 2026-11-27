// Real static Luau analyzer & cross-reference playtest prover for OpenRO's in-browser engine.
// Single source of truth shared with the Studio Explorer static diagnostics panel.
import { analyzePlaceLuau, type LuauDiagnostic } from "../src/app/luauAudit";
import type { PlaceState } from "./luau-skills";

export { analyzePlaceLuau, type LuauDiagnostic };

export function validatePlace(place: PlaceState): {
  passed: boolean;
  console: string;
  errors: string[];
  warnings: string[];
  diagnostics: LuauDiagnostic[];
} {
  const diagnostics = analyzePlaceLuau(place);
  const errors = diagnostics
    .filter((d) => d.severity === "error")
    .map((d) => `${d.path}:${d.line}: [${d.code}] ${d.message}`);
  const warnings = diagnostics
    .filter((d) => d.severity === "warning")
    .map((d) => `${d.path}:${d.line}: [${d.code}] ${d.message}`);

  if (errors.length === 0) {
    const warnNote = warnings.length ? `, ${warnings.length} advisory warning(s)` : "";
    return {
      passed: true,
      errors: [],
      warnings,
      diagnostics,
      console: `[OpenRO Playtest] Verified ${Object.keys(place.scripts).length} Luau script(s) and ${Object.keys(place.instances).length} Workspace instance(s): syntax OK, block balance OK, service roots OK, cross-references OK (0 errors${warnNote}).`,
    };
  }
  return {
    passed: false,
    errors,
    warnings,
    diagnostics,
    console: errors.join("\n"),
  };
}
