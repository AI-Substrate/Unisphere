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
  const cargo = envelope.data?.toolchain?.cargo?.provenance?.invoked ?? 'cargo';
  for (const name of ['composition', 'sdk-consumer', 'installed-cli', 'collection']) {
    const args = ['run', '--locked', '-p', 'unisphere-testkit', '--bin', 'unisphere-proof', '--', name];
    try {
      const result = await ctx.exec(cargo, args, { timeoutMs: name === 'collection' ? 600_000 : 300_000 });
      proofs.push({ name, command: cargo, args, ...result });
      if (!result.ok) return ctx.error('E_FOUNDATION_SMOKE', `${name} failed (exit ${result.code})`, {
        details: { checks: envelope, proofs }, next_action: `Run cargo ${args.join(' ')}, repair the failure, then rerun harness boot --json.`,
      });
    } catch (error) {
      return ctx.error('E_FOUNDATION_SMOKE_EXEC', `${name} could not execute: ${error.message}`, {
        details: { checks: envelope, proofs }, next_action: 'Repair the named smoke command or timeout; no foundation readiness was established.',
      });
    }
  }
  return ctx.ok({ ready: true, scope: 'configuration-and-claude-jsonl', checks: envelope, proofs,
    limitations: ['Explicit Unix JSONL loading and source-derived Claude records only; no universal session reconstruction or private-store discovery.', 'Metadata-only output is not anonymity: source paths and observed identities remain metadata.', 'No executed network-denial test; independent core/adapter source review is additionally required.'] });
}
