import assert from 'node:assert/strict';
import path from 'node:path';
import test from 'node:test';
import { fileURLToPath } from 'node:url';
import { Client } from '@modelcontextprotocol/client';
import { StdioClientTransport } from '@modelcontextprotocol/client/stdio';
import {
  buildVerifierInvocation,
  buildSourceListInvocation,
  normalizeCertificationInput,
  summarizeCertificationReport,
} from './subscription-verifier-mcp.mjs';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const serverPath = path.join(root, 'scripts', 'subscription-verifier-mcp.mjs');

test('certification summaries omit per-post evidence', () => {
  const summary = summarizeCertificationReport({
    site_id: 'gelbooru',
    query: 'artist',
    requested_first_fetch_source_posts: 100,
    first_fetch: {
      source_posts_processed: 100,
      materialized_post_count: 100,
      member_count: 101,
      first_materialized_post: { post_id: '200' },
      post_100: { post_id: '101' },
      last_materialized_post: { post_id: '101' },
      posts: [{ post_id: '200', members: [{ entity_hash: 'secretly-large' }] }],
    },
    final_state: { post_count: 101, member_count: 102 },
    checks: { restart_is_stable: true },
  });
  assert.equal(summary.firstFetch.firstMaterializedPostId, '200');
  assert.equal(summary.firstFetch.post100Id, '101');
  assert.equal(summary.firstFetch.lastMaterializedPostId, '101');
  assert.equal('posts' in summary.firstFetch, false);
});

test('MCP client lists the certification tool and rejects invalid input', async () => {
  const client = new Client({ name: 'picto-subscription-verifier-test', version: '1.0.0' });
  const transport = new StdioClientTransport({
    command: process.execPath,
    args: [serverPath],
    cwd: root,
    stderr: 'pipe',
  });

  await client.connect(transport);
  try {
    const listed = await client.listTools();
    assert.deepEqual(listed.tools.map(({ name }) => name), [
      'list_subscription_sources',
      'certify_subscription_source',
      'get_subscription_certification',
    ]);

    const sources = await client.callTool({
      name: 'list_subscription_sources',
      arguments: {},
    });
    assert.equal(sources.isError, false);
    assert.ok(Array.isArray(sources.structuredContent.sources));
    assert.ok(sources.structuredContent.sources.length > 0);
    assert.ok(sources.structuredContent.sources.some(({ site }) => site.id === 'pixiv'));

    const invalid = await client.callTool({
      name: 'certify_subscription_source',
      arguments: { site: 'gelbooru', query: 'known-query', postLimit: 0 },
    });
    assert.equal(invalid.isError, true);

    const defaults = normalizeCertificationInput({ site: 'gelbooru', query: 'known-query' });
    assert.equal(defaults.postLimit, 100);
    assert.equal(defaults.allowKeychain, false);
    assert.equal(defaults.credentialFile, undefined);
    assert.match(defaults.reportPath, /^artifacts\/subscription-certification\/gelbooru-/);

    const invocation = buildVerifierInvocation(defaults);
    assert.equal(invocation.command, process.execPath);
    assert.deepEqual(invocation.args.slice(1), [
      '--site',
      'gelbooru',
      '--query',
      'known-query',
      '--post-limit',
      '100',
      '--report',
      defaults.reportPath,
    ]);
    assert.equal(invocation.args.includes('--allow-keychain'), false);
    assert.equal(invocation.args.includes('--credential-file'), false);
    const fixtureInvocation = buildVerifierInvocation({
      site: 'gelbooru',
      query: 'known-query',
      credentialFile: '/tmp/picto-certification-credentials.json',
    });
    assert.deepEqual(fixtureInvocation.args.slice(-2), [
      '--credential-file',
      '/tmp/picto-certification-credentials.json',
    ]);
    assert.throws(
      () => normalizeCertificationInput({
        site: 'gelbooru',
        query: 'known-query',
        credentialFile: '/tmp/picto-certification-credentials.json',
        allowKeychain: true,
      }),
      /mutually exclusive/,
    );
    assert.throws(
      () => normalizeCertificationInput({
        site: 'gelbooru',
        query: 'known-query',
        reportPath: '../outside.json',
      }),
      /inside the Picto workspace/,
    );

    const sourceList = buildSourceListInvocation();
    assert.equal(sourceList.command, 'cargo');
    assert.deepEqual(sourceList.args, [
      'run',
      '--quiet',
      '--manifest-path',
      'core/Cargo.toml',
      '--example',
      'list_subscription_sources',
    ]);
  } finally {
    await client.close();
  }
});
