import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';

interface BuildConfig {
  directories?: { output?: string };
  nsis?: { oneClick?: boolean; allowToChangeInstallationDirectory?: boolean };
  linux?: { target?: string[]; artifactName?: string };
  files?: string[];
}

const pkg = JSON.parse(readFileSync('package.json', 'utf8')) as {
  author?: { name?: string; email?: string } | string;
  scripts?: Record<string, string>;
  dependencies?: Record<string, string>;
  devDependencies?: Record<string, string>;
  build?: BuildConfig;
  'electron-builder'?: unknown;
};

describe('packaging configuration', () => {
  it('uses the electron-builder "build" key so the settings are actually applied', () => {
    expect(pkg['electron-builder']).toBeUndefined();
    expect(pkg.build?.directories?.output).toBe('release');
    expect(pkg.build?.nsis?.oneClick).toBe(false);
    expect(pkg.build?.nsis?.allowToChangeInstallationDirectory).toBe(true);
  });

  it('keeps the documented packaging targets and bundled files', () => {
    expect(pkg.build?.linux?.target).toEqual(['AppImage', 'deb', 'pacman']);
    expect(pkg.build?.linux?.artifactName).toBe('open-ro-${version}-${arch}.${ext}');
    expect(pkg.build?.files).toEqual(['dist/**', 'dist-electron/**']);
  });

  it('declares an author email required by the deb and pacman targets', () => {
    expect(typeof pkg.author).toBe('object');
    expect((pkg.author as { email?: string }).email).toMatch(/^[^@\s]+@users\.noreply\.github\.com$/);
  });

  it('never publishes from a packaging script', () => {
    for (const name of ['package:win', 'package:linux', 'package:linux-omarchy']) {
      expect(pkg.scripts?.[name]).toContain('--publish never');
    }
  });

  it('keeps CI artifact paths aligned with the configured release directory', () => {
    const workflow = readFileSync('.github/workflows/ci.yml', 'utf8');
    expect(workflow).toContain('release/*.exe');
    expect(workflow).toContain('release/*.AppImage');
    expect(workflow).toContain('release/*.deb');
    expect(workflow).toContain('release/*.pacman');
    // fpm's pacman target shells out to bsdtar; the runner image does not ship it.
    expect(workflow).toContain('libarchive-tools');
  });
});
