import { defineExtension } from '@ai-substrate/engineering-harness/contract';
import { runChecks } from './checks.mjs';

export default defineExtension({
  name: 'checks',
  summary: 'Run the coherent Rust toolchain and foundation quality gates.',
  verbs: {
    'checks': {
      summary: 'Observe Rust tool identity, then run formatting, clippy, tests and architecture checks.',
      run: runChecks,
    },
  },
});
