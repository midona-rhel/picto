#!/usr/bin/env node

import { spawn } from 'node:child_process';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { McpServer } from '@modelcontextprotocol/server';
import { StdioServerTransport } from '@modelcontextprotocol/server/stdio';
import { z } from 'zod';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const verifier = path.join(root, 'scripts', 'verify-sites.mjs');
const maxOutputLength = 8_000;

export const certificationInputSchema = z.object({
  site: z.string().trim().min(1),
  query: z.string().trim().min(1),
  postLimit: z.number().int().min(1).default(100),
  credentialFile: z.string().trim().min(1).optional(),
  allowKeychain: z.boolean().default(false),
  reportPath: z.string().trim().min(1).optional(),
}).refine((input) => !(input.credentialFile && input.allowKeychain), {
  message: 'credentialFile and allowKeychain are mutually exclusive',
});

export function normalizeCertificationInput(input) {
  const parsed = certificationInputSchema.parse(input);
  const timestamp = new Date().toISOString().replaceAll(/[:.]/g, '-');
  const safeSite = parsed.site.replaceAll(/[^a-zA-Z0-9_-]/g, '_');
  const reportPath = parsed.reportPath
    ?? path.join('artifacts', 'subscription-certification', `${safeSite}-${timestamp}.json`);
  const absoluteReportPath = path.resolve(root, reportPath);
  if (!absoluteReportPath.startsWith(`${root}${path.sep}`)) {
    throw new Error('reportPath must stay inside the Picto workspace');
  }
  return {
    ...parsed,
    reportPath: path.relative(root, absoluteReportPath),
  };
}

export function buildVerifierInvocation(input) {
  const normalized = normalizeCertificationInput(input);
  const args = [
    verifier,
    '--site',
    normalized.site,
    '--query',
    normalized.query,
    '--post-limit',
    String(normalized.postLimit),
    '--report',
    normalized.reportPath,
  ];
  if (normalized.credentialFile) args.push('--credential-file', normalized.credentialFile);
  if (normalized.allowKeychain) args.push('--allow-keychain');

  return {
    command: process.execPath,
    args,
    cwd: root,
    env: {
      ...process.env,
    },
    input: normalized,
  };
}

export function buildSourceListInvocation() {
  return {
    command: 'cargo',
    args: [
      'run',
      '--quiet',
      '--manifest-path',
      'core/Cargo.toml',
      '--example',
      'list_subscription_sources',
    ],
    cwd: root,
    env: { ...process.env },
  };
}

function boundOutput(value) {
  if (value.length <= maxOutputLength) return value;
  return `${value.slice(-maxOutputLength)}\n[output truncated]`;
}

export function summarizeCertificationReport(report) {
  if (!report) return null;
  return {
    siteId: report.site_id,
    query: report.query,
    requestedFirstFetchSourcePosts: report.requested_first_fetch_source_posts,
    firstFetch: {
      sourcePostsProcessed: report.first_fetch?.source_posts_processed,
      materializedPostCount: report.first_fetch?.materialized_post_count,
      memberCount: report.first_fetch?.member_count,
      firstMaterializedPostId: report.first_fetch?.first_materialized_post?.post_id,
      post100Id: report.first_fetch?.post_100?.post_id,
      lastMaterializedPostId: report.first_fetch?.last_materialized_post?.post_id,
    },
    finalState: report.final_state,
    checks: report.checks,
  };
}

export function runVerifier(input) {
  const invocation = buildVerifierInvocation(input);

  return new Promise((resolve, reject) => {
    const child = spawn(invocation.command, invocation.args, {
      cwd: invocation.cwd,
      env: invocation.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';

    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code, signal) => {
      let reportSummary = null;
      let reportError = null;
      if (code === 0) {
        try {
          const report = JSON.parse(fs.readFileSync(
            path.resolve(root, invocation.input.reportPath),
            'utf8',
          ));
          reportSummary = summarizeCertificationReport(report);
        } catch (error) {
          reportError = `Verifier exited successfully but its report is unavailable: ${error}`;
        }
      }
      resolve({
        exitCode: code,
        signal,
        reportPath: invocation.input.reportPath,
        reportSummary,
        reportError,
        stdout: boundOutput(stdout),
        stderr: boundOutput(stderr),
      });
    });
  });
}

export function runSourceList() {
  const invocation = buildSourceListInvocation();

  return new Promise((resolve, reject) => {
    const child = spawn(invocation.command, invocation.args, {
      cwd: invocation.cwd,
      env: invocation.env,
      stdio: ['ignore', 'pipe', 'pipe'],
    });
    let stdout = '';
    let stderr = '';

    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => { stdout += chunk; });
    child.stderr.on('data', (chunk) => { stderr += chunk; });
    child.once('error', reject);
    child.once('close', (code, signal) => resolve({
      exitCode: code,
      signal,
      stdout,
      stderr: boundOutput(stderr),
    }));
  });
}

export function createSubscriptionVerifierServer({ execute = runVerifier } = {}) {
  const server = new McpServer({
    name: 'picto-subscription-verifier',
    version: '1.0.0',
  });
  const certificationJobs = new Map();

  server.registerTool(
    'list_subscription_sources',
    {
      title: 'List Picto subscription sources',
      description: 'Read-only listing of subscription sources exposed by Picto\'s backend registry.',
    },
    async () => {
      try {
        const result = await runSourceList();
        const sources = result.exitCode === 0 ? JSON.parse(result.stdout) : [];
        const output = {
          passed: result.exitCode === 0,
          exitCode: result.exitCode,
          signal: result.signal,
          stderr: result.stderr,
          sources,
        };
        return {
          isError: !output.passed,
          content: [{ type: 'text', text: JSON.stringify(output, null, 2) }],
          structuredContent: output,
        };
      } catch (error) {
        return {
          isError: true,
          content: [{ type: 'text', text: `Failed to launch source listing: ${error}` }],
        };
      }
    },
  );

  server.registerTool(
    'certify_subscription_source',
    {
      title: 'Certify Picto subscription source',
      description: 'Run Picto\'s production-backed subscription certification for one source query.',
      inputSchema: certificationInputSchema,
    },
    async (input) => {
      try {
        const normalized = normalizeCertificationInput(input);
        const jobId = crypto.randomUUID();
        const started = {
          jobId,
          status: 'running',
          site: normalized.site,
          query: normalized.query,
          postLimit: normalized.postLimit,
          credentialFile: normalized.credentialFile,
          allowKeychain: normalized.allowKeychain,
          reportPath: normalized.reportPath,
        };
        certificationJobs.set(jobId, started);
        Promise.resolve(execute(normalized)).then(
          (result) => {
            certificationJobs.set(jobId, {
              ...started,
              status: result.exitCode === 0 && result.reportSummary != null ? 'passed' : 'failed',
              exitCode: result.exitCode,
              signal: result.signal,
              reportSummary: result.reportSummary,
              reportError: result.reportError,
              stdout: result.stdout,
              stderr: result.stderr,
            });
          },
          (error) => {
            certificationJobs.set(jobId, {
              ...started,
              status: 'failed',
              error: String(error),
            });
          },
        );
        return {
          content: [{ type: 'text', text: JSON.stringify(started, null, 2) }],
          structuredContent: started,
        };
      } catch (error) {
        return {
          isError: true,
          content: [{ type: 'text', text: `Failed to launch subscription verifier: ${error}` }],
        };
      }
    },
  );

  server.registerTool(
    'get_subscription_certification',
    {
      title: 'Get Picto subscription certification',
      description: 'Read the current state and final evidence of a certification job.',
      inputSchema: z.object({ jobId: z.string().uuid() }),
    },
    async ({ jobId }) => {
      const job = certificationJobs.get(jobId);
      if (!job) {
        return {
          isError: true,
          content: [{ type: 'text', text: `Unknown certification job: ${jobId}` }],
        };
      }
      return {
        isError: job.status === 'failed',
        content: [{ type: 'text', text: JSON.stringify(job, null, 2) }],
        structuredContent: job,
      };
    },
  );

  return server;
}

async function main() {
  const server = createSubscriptionVerifierServer();
  await server.connect(new StdioServerTransport());
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((error) => {
    console.error(error);
    process.exitCode = 1;
  });
}
