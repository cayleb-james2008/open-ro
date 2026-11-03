/** Environment variables a project tool or MCP server may inherit. Secrets are not on this list. */
const ALLOWED_ENV = [
  'PATH',
  'HOME',
  'USERPROFILE',
  'APPDATA',
  'LOCALAPPDATA',
  'SystemRoot',
  'COMSPEC',
  'WINDIR',
  'PATHEXT',
  'SHELL',
  'LANG',
  'LC_ALL',
  'TMP',
  'TEMP',
  'TMPDIR',
] as const;

export function scrubbedEnv(source: NodeJS.ProcessEnv = process.env): NodeJS.ProcessEnv {
  const env: NodeJS.ProcessEnv = {};
  for (const key of ALLOWED_ENV) {
    const value = source[key];
    if (typeof value === 'string' && value.length > 0) {
      env[key] = value;
    }
  }
  return env;
}
