import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import test from 'node:test';
import { runBoot } from './boot.mjs';

function context(exec, exists = true) {
  return {
    fs: { exists: () => exists }, exec,
    ok: data => ({ status: 'ok', data }),
    degraded: (data, next_action) => ({ status: 'degraded', data, next_action }),
    unconfigured: next_action => ({ status: 'unconfigured', next_action }),
    error: (code, message, extra) => ({ status: 'error', error: { code, message, ...extra } }),
  };
}
const good = { ok: true, code: 0, stdout: '{"command":"checks","status":"ok"}', stderr: '' };

test('missing, unconfigured, malformed and degraded gates cannot imply readiness', async () => {
  const missing = await runBoot(context(() => assert.fail('must not spawn'), false));
  assert.equal(missing.data.ready, false);
  for (const [result, status] of [
    [{ ok: false, code: 2, stdout: '', stderr: '' }, 'unconfigured'],
    [{ ...good, stdout: 'not json' }, 'error'],
    [{ ...good, stdout: '{"command":"checks","status":"degraded"}' }, 'degraded'],
    [{ ...good, stdout: '{"command":"other","status":"ok"}' }, 'degraded'],
  ]) {
    let calls = 0;
    const verdict = await runBoot(context(async () => { calls += 1; return result; }));
    assert.equal(verdict.status, status);
    assert.equal(calls, 1);
    assert.notEqual(verdict.data?.ready, true);
  }
});

test('failed product checks preserve exit/stdout/stderr and never start smoke', async () => {
  let calls = 0;
  const verdict = await runBoot(context(async () => { calls += 1; return { ok: false, code: 17, stdout: 'output', stderr: 'failure' }; }));
  assert.equal(calls, 1);
  assert.equal(verdict.error.code, 'E_CHECKS_FAILED');
  assert.equal(verdict.error.details.code, 17);
  assert.equal(verdict.error.details.stdout, 'output');
  assert.equal(verdict.error.details.stderr, 'failure');
});

test('each required smoke failure prevents readiness and preserves failure evidence', async () => {
  for (let failed = 0; failed < 4; failed += 1) {
    let calls = 0;
    const verdict = await runBoot(context(async () => {
      const index = calls++;
      if (index === 0) return good;
      return index === failed + 1 ? { ok: false, code: 19, stdout: 'smoke-out', stderr: 'smoke-error' } : { ...good, stdout: 'passed' };
    }));
    assert.equal(verdict.error.code, 'E_FOUNDATION_SMOKE');
    assert.equal(calls, failed + 2);
    assert.equal(verdict.error.details.proofs.at(-1).stderr, 'smoke-error');
  }
});

test('only complete quality and assembled proofs establish foundation readiness', async () => {
  const calls = [];
  const verdict = await runBoot(context(async (command, args) => { calls.push([command, args]); return good; }));
  assert.equal(verdict.data.ready, true);
  assert.equal(verdict.data.scope, 'configuration-and-claude-jsonl');
  assert.equal(calls.length, 5);
  assert.deepEqual(calls.slice(1).map(([, args]) => args.at(-1)), ['composition', 'sdk-consumer', 'installed-cli', 'collection']);
});

test('a real harness child failure is not laundered into readiness', () => {
  const cwd = mkdtempSync(join(tmpdir(), 'unisphere-boot-'));
  try {
    const boot = join(cwd, '.harness/extensions/boot');
    const checks = join(cwd, '.harness/extensions/checks');
    mkdirSync(boot, { recursive: true });
    mkdirSync(checks, { recursive: true });
    for (const file of ['extension.ts', 'boot.mjs', 'instructions.md']) copyFileSync(new URL(`./${file}`, import.meta.url), join(boot, file));
    writeFileSync(join(checks, 'extension.js'), `export default { kind: 'extension', name: 'checks', summary: 'Controlled failure', verbs: { checks: { summary: 'Fail product check', run(ctx) { return ctx.error('E_FIXTURE', 'fixture failure', { next_action: 'Repair fixture.' }); } } } };\n`);
    writeFileSync(join(checks, 'instructions.md'), '# Controlled harness fixture\n');
    const result = spawnSync('harness', ['boot', '--json'], { cwd, encoding: 'utf8', timeout: 15_000, env: { ...process.env, HARNESS_NO_TELEMETRY: '1', HARNESS_NO_TELEMETRY_AUTOSYNC: '1' } });
    assert.ifError(result.error);
    assert.equal(result.status, 1, result.stderr || result.stdout);
    const envelope = JSON.parse(result.stdout);
    assert.equal(envelope.error.code, 'E_CHECKS_FAILED');
    assert.match(envelope.error.details.stdout, /fixture failure/);
  } finally { rmSync(cwd, { recursive: true, force: true }); }
});
