// Tailwind v3, pinned — the FRONTEND-LIBRARY standard's compiler config, carried into Open-Ro.
// Source: openbell-watchover/design/watchover/tailwind.config.cjs (the design pack the fleet's
// house style lives in), with only `content` widened to this app's source extensions. The theme is
// generated from tokens/tokens.json, so no default Tailwind colour, shadow, ramp or opacity value
// can reach the compiled CSS; every core plugin that emits a ramp, shadow, opacity, filter, blend
// or a --tw-* variable stays off (the same list the pack ships and the same strip in postcss).
const theme = require('./src/house/tokens/tailwind-theme.json');

module.exports = {
  content: ['./index.html', './src/**/*.{js,jsx,ts,tsx,html}'],
  theme: {
    colors: theme.colors,
    fontFamily: theme.fontFamily,
    fontSize: theme.fontSize,
    fontWeight: theme.fontWeight,
    letterSpacing: theme.letterSpacing,
    screens: theme.screens,
    transitionDuration: theme.transitionDuration,
    transitionTimingFunction: theme.transitionTimingFunction,
    zIndex: theme.zIndex,
    extend: {
      spacing: theme.spacing,
      lineHeight: theme.lineHeight,
      borderRadius: theme.borderRadius,
      minWidth: theme.minWidth,
      width: theme.width,
      height: theme.height,
      minHeight: theme.minHeight,
      maxHeight: theme.maxHeight,
      borderWidth: theme.borderWidth,
      outlineWidth: theme.outlineWidth,
      outlineOffset: theme.outlineOffset,
    },
  },
  corePlugins: {
    preflight: false, // the pack ships its own reset; Tailwind's placeholder grey is not in the palette
    ringWidth: false, ringColor: false, ringOffsetWidth: false, ringOffsetColor: false, ringOpacity: false,
    boxShadow: false, boxShadowColor: false, dropShadow: false,
    gradientColorStops: false, backgroundImage: false,
    opacity: false, backgroundOpacity: false, textOpacity: false, borderOpacity: false, divideOpacity: false, placeholderOpacity: false,
    blur: false, backdropBlur: false, backdropFilter: false, filter: false,
    brightness: false, contrast: false, grayscale: false, hueRotate: false, invert: false, saturate: false, sepia: false,
    backdropBrightness: false, backdropContrast: false, backdropGrayscale: false, backdropHueRotate: false, backdropInvert: false,
    backdropOpacity: false, backdropSaturate: false, backdropSepia: false,
    mixBlendMode: false, backgroundBlendMode: false,
    transitionProperty: false, // the house motion is hand-written (three motions, no others)
    transform: false, translate: false, rotate: false, skew: false, scale: false,
    touchAction: false, scrollSnapType: false, borderSpacing: false, fontVariantNumeric: false,
    animation: false, // no keyframes in a page with no idle motion
  },
};
