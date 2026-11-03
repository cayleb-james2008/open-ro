import { configDefaults, defineConfig } from 'vitest/config';

// tests/luau runs generated Luau with a Luau CLI, so it runs only when OPENRO_LUAU_BIN names one.
const luau = Boolean(process.env.OPENRO_LUAU_BIN);

export default defineConfig({
  test: {
    include: ['tests/**/*.test.ts'],
    exclude: luau ? configDefaults.exclude : [...configDefaults.exclude, 'tests/luau/**'],
  },
});
