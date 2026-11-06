/**
 * The agent presence mark: a flat, monochrome neural-net glyph (nodes and
 * connecting lines, no character, no face), drawn for Open-Ro in the Ergon
 * face grammar. It never animates; whether the agent is working is said in
 * words beside it, not by making the mark move.
 */
export function PresenceMark({ size = 30 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="0 0 32 32" fill="none" aria-hidden="true" className="presence-mark">
      <path d="M10 7 L16 16 M22 5 L16 16 M6 16 H16 M26 17 H16 M10 26 L16 16 M23 25 L16 16 M16 16 V28" stroke="currentColor" strokeWidth="1.1" opacity="0.55" />
      <circle cx="16" cy="16" r="3.1" fill="currentColor" />
      <circle cx="10" cy="7" r="1.9" fill="currentColor" opacity="0.8" />
      <circle cx="22" cy="5" r="1.7" fill="currentColor" opacity="0.65" />
      <circle cx="6" cy="16" r="1.8" fill="currentColor" opacity="0.75" />
      <circle cx="26" cy="17" r="1.8" fill="currentColor" opacity="0.75" />
      <circle cx="10" cy="26" r="1.7" fill="currentColor" opacity="0.65" />
      <circle cx="23" cy="25" r="1.9" fill="currentColor" opacity="0.8" />
      <circle cx="16" cy="28" r="1.5" fill="currentColor" opacity="0.55" />
    </svg>
  );
}
