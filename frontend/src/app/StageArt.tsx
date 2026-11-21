// One small picture per Quest stage, drawn in the palette tokens (from the Quest prototype).
const F = (v: string) => ({ fill: `var(${v})` });
const S = (v: string) => ({ stroke: `var(${v})`, fill: "none" });

export function StageArt({ stage }: { stage: number }) {
  if (stage === 0) {
    return (
      <svg viewBox="0 0 240 120" aria-hidden="true" className="h-full w-full">
        <rect x="24" y="24" width="60" height="72" rx="10" style={{ ...F("--surface"), stroke: "var(--hairline)" }} />
        <rect x="90" y="16" width="60" height="88" rx="10" style={{ ...F("--surface"), stroke: "var(--blue)", strokeWidth: 3 }} />
        <rect x="156" y="24" width="60" height="72" rx="10" style={{ ...F("--surface"), stroke: "var(--hairline)" }} />
      </svg>
    );
  }
  if (stage === 1) {
    return (
      <svg viewBox="0 0 240 120" aria-hidden="true" className="h-full w-full">
        <rect x="20" y="26" width="200" height="72" rx="14" style={F("--ink-2")} />
        {[0, 1, 2].map((r) => [0, 1, 2, 3, 4, 5, 6].map((c) => (
          <rect key={`${r}${c}`} x={34 + c * 26} y={38 + r * 18} width="18" height="11" rx="3" style={F(r === 1 && c === 3 ? "--blue" : "--surface")} />
        )))}
      </svg>
    );
  }
  if (stage === 2) {
    return (
      <svg viewBox="0 0 240 120" aria-hidden="true" className="h-full w-full">
        <rect x="40" y="82" width="70" height="26" rx="5" style={F("--ink")} />
        <rect x="112" y="82" width="70" height="26" rx="5" style={F("--ink-2")} />
        <rect x="76" y="54" width="70" height="26" rx="5" style={F("--blue")} />
        <rect x="150" y="54" width="46" height="26" rx="5" style={F("--ink-3")} />
        <rect x="116" y="28" width="44" height="24" rx="5" style={F("--ink")} />
      </svg>
    );
  }
  if (stage === 3) {
    return (
      <svg viewBox="0 0 240 120" aria-hidden="true" className="h-full w-full">
        <circle cx="120" cy="60" r="46" style={{ ...F("--surface"), stroke: "var(--ink)", strokeWidth: 3 }} />
        <polygon points="108,38 108,82 146,60" style={F("--blue")} />
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 240 120" aria-hidden="true" className="h-full w-full">
      <rect x="62" y="18" width="116" height="84" rx="12" style={{ ...F("--surface"), stroke: "var(--ink)", strokeWidth: 3 }} />
      <path d="M92 60 H148 M130 42 L150 60 L130 78" style={S("--blue")} strokeWidth="6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
