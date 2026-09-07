export async function runBoot(ctx) {
  if (!ctx.fs.exists('.harness/extensions/checks')) {
    return ctx.degraded({ ready: false }, 'Restore the checks extension before claiming foundation readiness.');
  }
  let checks;
  try {
    checks = await ctx.exec('harness', ['checks', '--json'], { timeoutMs: 480_000 });
  } catch (error) {
    return ctx.error('E_CHECKS_EXEC', `Checks could not execute: ${error.message}`, { next_action: 'Repair checks execution, then rerun harness boot --json.' });
  }
  if (checks.code === 2) return ctx.unconfigured('The product quality gate is unconfigured. Implement the canonical product lane before claiming foundation readiness.');
  if (!checks.ok) {
    return ctx.error('E_CHECKS_FAILED', `harness checks failed (exit ${checks.code})`, {
      details: { ...checks }, next_action: 'Run harness checks --json, repair its reported failure, then rerun harness boot --json.',
    });
  }
  let envelope;
  try { envelope = JSON.parse(checks.stdout); } catch {
    return ctx.error('E_CHECKS_ENVELOPE', 'Checks returned invalid JSON.', { details: { ...checks }, next_action: 'Repair checks envelope output; exit zero alone is not readiness.' });
  }
  if (envelope.command !== 'checks' || envelope.status !== 'ok') {
    return ctx.degraded({ ready: false, checks: envelope }, 'Resolve the non-ok quality gate before running foundation smoke.');
  }
  const proofs = [];
  for (const name of ['composition', 'sdk-consumer', 'installed-cli']) {
    const args = ['run', '--locked', '-p', 'unisphere-testkit', '--bin', 'unisphere-proof', '--', name];
    try {
      const result = await ctx.exec('cargo', args, { timeoutMs: 300_000 });
      proofs.push({ name, command: 'cargo', args, ...result });
      if (!result.ok) return ctx.error('E_FOUNDATION_SMOKE', `${name} failed (exit ${result.code})`, {
        details: { checks: envelope, proofs }, next_action: `Run cargo ${args.join(' ')}, repair the failure, then rerun harness boot --json.`,
      });
    } catch (error) {
      return ctx.error('E_FOUNDATION_SMOKE_EXEC', `${name} could not execute: ${error.message}`, {
        details: { checks: envelope, proofs }, next_action: 'Repair the named smoke command or timeout; no foundation readiness was established.',
      });
    }
  }
  return ctx.ok({ ready: true, scope: 'configuration-sdk-cli-foundation', checks: envelope, proofs,
    limitations: ['No native telemetry reader or collection readiness.', 'No real terminal detection proof.', 'No executed network-denial test; independent core/SDK source review is additionally required.'] });
}
