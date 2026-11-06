// Tailwind, then the FRONTEND-LIBRARY standard's SPEC 11.2 strip: the `*, ::before, ::after { --tw-… }`
// defaults block and any stray --tw-* declaration are removed, and the build fails if any remaining
// declaration still reads a --tw-* variable (a utility that would silently stop working).
// Source: openbell-watchover/design/watchover/postcss.config.cjs (carried verbatim except this comment).
const tailwind = require('tailwindcss');

const stripTwVars = () => ({
  postcssPlugin: 'watchover-strip-tw-vars',
  OnceExit(root) {
    root.walkDecls((d) => { if (d.prop.startsWith('--tw-')) d.remove(); });
    root.walkRules((r) => { if (!r.nodes || r.nodes.length === 0) r.remove(); });
    root.walkAtRules((a) => { if (a.nodes && a.nodes.length === 0) a.remove(); });
    const bad = [];
    root.walkDecls((d) => { if (d.value.includes('var(--tw-')) bad.push(`${d.parent.selector} { ${d.prop}: ${d.value} }`); });
    if (bad.length) throw root.error(`compiled CSS still reads --tw-* variables after the strip; turn the core plugin off in tailwind.config.cjs and hand-write the utility:\n${bad.join('\n')}`);
  },
});
stripTwVars.postcss = true;

module.exports = { plugins: [tailwind(), stripTwVars()] };
