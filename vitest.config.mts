import { configDefaults, defineConfig } from 'vitest/config';

// tests/integration needs a real MCP server, so it runs only with OPENRO_MCP_INTEGRATION=1.
const integration = process.env.OPENRO_MCP_INTEGRATION === '1';
// tests/luau runs generated Luau with a Luau CLI, so it runs only when OPENRO_LUAU_BIN names one.
const luau = Boolean(process.env.OPENRO_LUAU_BIN);

export default defineConfig({
  test: {
    include: ['tests/**/*.test.ts'],
    exclude: [...configDefaults.exclude, ...(integration ? [] : ['tests/integration/**']), ...(luau ? [] : ['tests/luau/**'])],
  },
});
