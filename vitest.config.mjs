import { defineConfig } from 'vitest/config';

// Runs the tests of the scripts in `scripts/`.
// `subscription-verifier-mcp.test.mjs` is written for Node's own test runner
// (`npm run test:subscriptions:verify-mcp`) and is left out here.
export default defineConfig({
  test: {
    include: ['scripts/**/*.test.mjs'],
    exclude: ['scripts/subscription-verifier-mcp.test.mjs'],
  },
});
