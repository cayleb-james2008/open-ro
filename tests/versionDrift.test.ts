import { execSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';
import {
  checkBlenderPipelineDrift,
  checkProjectVersionDrift,
  checkProviderCatalogDrift,
  checkStudioToolsDrift,
  checkTestSuiteDrift,
  checkToolchainDrift,
  runVersionDriftAudit,
  type PackageJsonStructure,
} from '../electron/versionDrift';

const repoRoot = path.resolve(__dirname, '..');
const pkg = JSON.parse(readFileSync(path.join(repoRoot, 'package.json'), 'utf8')) as PackageJsonStructure;

describe('Documentation and integration version drift audit (electron/versionDrift.ts)', () => {
  it('passes cleanly on the live repository with zero drift across all checked touchpoints', async () => {
    const report = await runVersionDriftAudit(repoRoot);
    expect(report.passed).toBe(true);
    expect(report.driftCount).toBe(0);
    expect(report.totalChecks).toBeGreaterThanOrEqual(20);
    expect(report.summary).toMatch(/passed cleanly/);
    expect(report.drifts).toHaveLength(0);
  });

  it('verifies project version matches CHANGELOG, BUILD-STATUS, and RELEASE-GATES', async () => {
    const checks = await checkProjectVersionDrift(repoRoot, pkg);
    expect(checks.length).toBeGreaterThanOrEqual(3);
    for (const check of checks) {
      expect(check.status).toBe('ok');
    }
  });

  it('detects simulated project version drift when package.json version is mutated', async () => {
    const mutatedPkg: PackageJsonStructure = {
      ...pkg,
      version: '99.99.99',
    };
    const checks = await checkProjectVersionDrift(repoRoot, mutatedPkg);
    const drifts = checks.filter((c) => c.status === 'drift');
    expect(drifts.length).toBeGreaterThanOrEqual(2);
    expect(drifts.some((d) => d.identifier.includes('BUILD-STATUS.md'))).toBe(true);
    expect(drifts.some((d) => d.identifier.includes('CHANGELOG.md'))).toBe(true);
  });

  it('verifies engine and toolchain alignment between package.json and architectural docs', async () => {
    const checks = await checkToolchainDrift(repoRoot, pkg);
    expect(checks.length).toBeGreaterThanOrEqual(3);
    for (const check of checks) {
      expect(check.status).toBe('ok');
    }
  });

  it('detects simulated toolchain drift when dependency versions are mutated', async () => {
    const mutatedPkg: PackageJsonStructure = {
      ...pkg,
      devDependencies: {
        ...pkg.devDependencies,
        electron: '^99.0.0',
      },
      dependencies: {
        ...pkg.dependencies,
        react: '^99.0.0',
      },
    };
    const checks = await checkToolchainDrift(repoRoot, mutatedPkg);
    const electronDrift = checks.find((c) => c.identifier === 'ElectronMajorVersion');
    const reactDrift = checks.find((c) => c.identifier === 'ReactMajorVersion');

    expect(electronDrift?.status).toBe('drift');
    expect(reactDrift?.status).toBe('drift');
  });

  it('verifies provider catalog coverage and data egress classification', async () => {
    const checks = await checkProviderCatalogDrift(repoRoot);
    expect(checks.length).toBeGreaterThanOrEqual(9);
    for (const check of checks) {
      expect(check.status).toBe('ok');
    }
    const dataFlow = checks.find((c) => c.identifier === 'DataFlowDisclosureCoverage');
    expect(dataFlow?.status).toBe('ok');
  });

  it('verifies all native Studio MCP tools are registered and documented', async () => {
    const checks = await checkStudioToolsDrift(repoRoot);
    expect(checks.length).toBeGreaterThanOrEqual(5);
    for (const check of checks) {
      expect(check.status).toBe('ok');
    }
    expect(checks.some((c) => c.identifier.includes('studio_search_tree'))).toBe(true);
    expect(checks.some((c) => c.identifier.includes('studio_multi_edit'))).toBe(true);
  });

  it('verifies all 5 core Blender 3D pipeline tasks are typed and implemented', async () => {
    const checks = await checkBlenderPipelineDrift(repoRoot);
    expect(checks).toHaveLength(5);
    for (const check of checks) {
      expect(check.status).toBe('ok');
    }
    const taskNames = checks.map((c) => c.identifier);
    expect(taskNames).toContain('blenderTask:inspect_mesh');
    expect(taskNames).toContain('blenderTask:convert_format');
    expect(taskNames).toContain('blenderTask:decimate_mesh');
    expect(taskNames).toContain('blenderTask:generate_collision');
    expect(taskNames).toContain('blenderTask:export_roblox_package');
  });

  it('verifies active test suites satisfy documented baseline without regression', async () => {
    const checks = await checkTestSuiteDrift(repoRoot);
    expect(checks).toHaveLength(1);
    expect(checks[0].status).toBe('ok');
    expect(checks[0].identifier).toBe('TestSuiteCountRegressions');
  });

  it('runs scripts/check-drift.mjs CLI directly with exit code 0', () => {
    const output = execSync('node scripts/check-drift.mjs', {
      cwd: repoRoot,
      encoding: 'utf8',
    });
    expect(output).toContain('Drift check passed');
    expect(output).toContain('0 drift');
  });

  it('wires check:drift into package.json scripts and npm run lint', () => {
    const rawPkg = JSON.parse(readFileSync(path.join(repoRoot, 'package.json'), 'utf8')) as {
      scripts: Record<string, string>;
    };
    expect(rawPkg.scripts['check:drift']).toBe('node scripts/check-drift.mjs');
    expect(rawPkg.scripts.lint).toContain('node scripts/check-drift.mjs');
  });
});
