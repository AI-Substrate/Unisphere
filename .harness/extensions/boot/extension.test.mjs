import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';

// These exercise harness composition only, never claim to test the collector.
const cases = [
  { name: 'missing checks cannot imply readiness', gate: null, status: 'degraded', exit: 0 },
  { name: 'the real unconfigured gate stays unconfigured', gate: 'actual', status: 'unconfigured', exit: 2 },
  { name: 'failed checks preserve diagnostics', gate: "ctx.error('E_FIXTURE', 'fixture failure', { next_action: 'Repair the fixture gate.' })", status: 'error', exit: 1 },
  { name: 'passing checks alone cannot imply readiness', gate: 'ctx.ok({ passed: true })', status: 'degraded', exit: 0 },
  { name: 'degraded checks cannot imply readiness', gate: "ctx.degraded({ passed: false }, 'Resolve the fixture warning.')", status: 'degraded', exit: 0 },
];

for (const scenario of cases) {
  test(scenario.name, () => {
    const cwd = mkdtempSync(join(tmpdir(), 'unisphere-boot-'));
    try {
      const boot = join(cwd, '.harness/extensions/boot');
      mkdirSync(boot, { recursive: true });
      copyFileSync(new URL('./extension.ts', import.meta.url), join(boot, 'extension.ts'));
      copyFileSync(new URL('./instructions.md', import.meta.url), join(boot, 'instructions.md'));
      if (scenario.gate !== null) {
        const checks = join(cwd, '.harness/extensions/checks');
        mkdirSync(checks);
        if (scenario.gate === 'actual') {
          copyFileSync(new URL('../checks/extension.ts', import.meta.url), join(checks, 'extension.ts'));
        } else {
          writeFileSync(join(checks, 'extension.js'), `export default {
            kind: 'extension', name: 'checks', summary: 'Isolated test fixture',
            verbs: { checks: { summary: 'Fixture quality gate', run(ctx) { return ${scenario.gate}; } } }
          };\n`);
        }
        writeFileSync(join(checks, 'instructions.md'), '# Isolated harness test fixture\n');
      }
      const result = spawnSync('harness', ['boot', '--json'], {
        cwd, encoding: 'utf8', timeout: 15_000,
        env: { ...process.env, HARNESS_NO_TELEMETRY: '1', HARNESS_NO_TELEMETRY_AUTOSYNC: '1' },
      });
      assert.ifError(result.error);
      assert.equal(result.status, scenario.exit, result.stderr || result.stdout);
      const envelope = JSON.parse(result.stdout);
      assert.equal(envelope.command, 'boot');
      assert.equal(envelope.status, scenario.status);
      assert.ok(envelope.next_action);
      if (scenario.status === 'degraded') assert.equal(envelope.data.ready, false);
      if (scenario.gate === 'actual') assert.match(envelope.next_action, /product build\/test lane/i);
      if (scenario.status === 'error') {
        assert.equal(envelope.error.code, 'E_CHECKS_FAILED');
        assert.match(envelope.error.details.stdout, /fixture failure/);
      }
      if (scenario.gate?.startsWith('ctx.ok')) assert.equal(envelope.data.checks.status, 'ok');
      if (scenario.gate?.startsWith('ctx.degraded')) assert.equal(envelope.data.checks.status, 'degraded');
    } finally {
      rmSync(cwd, { recursive: true, force: true });
    }
  });
}
