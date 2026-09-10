import { defineExtension } from '@ai-substrate/engineering-harness/contract';
import { runBoot } from './boot.mjs';

export default defineExtension({
  name: 'boot',
  summary: 'Report Unisphere readiness through its product quality gate.',
  verbs: {
    'boot': {
      summary: 'Run quality, native-session and read-only Git Notes consumer proofs without starting services.',
      run: runBoot,
    },
  },
});
