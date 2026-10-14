// Public SDK + real WASM + real filesystem. Only the remote model is synthetic.
// Run: node --test js/test/claude-workspace-wasm-journey.test.mjs
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { createServer } from 'node:http';
import { mkdtemp, mkdir, readFile, writeFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { Nanocodex, Transport } from '../host/index.mjs';
import { open } from '../node/workspace.mjs';
import { codeEvaluator } from './quickjs-fixture.mjs';

function stream(block) {
  return [
    { type: 'message_start', message: { id: 'workspace-fixture', role: 'assistant', model: 'fixture', content: [], usage: { input_tokens: 10, output_tokens: 0 } } },
    { type: 'content_block_start', index: 0, content_block: block },
    { type: 'content_block_stop', index: 0 },
    { type: 'message_delta', delta: { stop_reason: block.type === 'text' ? 'end_turn' : 'tool_use' }, usage: { output_tokens: 1 } },
    { type: 'message_stop' },
  ].map(frame => `data: ${JSON.stringify(frame)}\n\n`).join('');
}

for (const codeMode of [false, true]) {
  test(`Claude defaults edit a shared workspace across roots (codeMode=${codeMode})`, { timeout: 60_000 }, async t => {
    const directory = await mkdtemp(join(tmpdir(), 'claude-workspace-'));
    t.after(() => rm(directory, { recursive: true, force: true }));
    const filesystem = await open({ path: directory, root: '/project' });
    await filesystem.writeFile('shared.txt', 'red\nblue\nrepeat repeat\n');
    const steps = new Map(), trace = [], errors = [];
    const scripts = {
      FIRST: [
        ['Read', { file_path: '/project/shared.txt', offset: 2, limit: 1 }, /2\tblue/],
        ['Edit', { file_path: 'shared.txt', old_string: 'red', new_string: 'green' }, /Updated shared.txt/],
        ['Write', { file_path: 'nested/new.txt', content: 'new content\n' }, /Wrote nested\/new.txt/],
        ['Glob', { pattern: '**/*.txt' }, /nested\/new.txt/],
        ['Grep', { pattern: '^(green|bl[u]+e)$', output_mode: 'content' }, /shared.txt:1:green[\s\S]*shared.txt:2:blue/],
      ],
      SECOND: [
        ['Read', { file_path: 'shared.txt' }, /1\tgreen/],
        ['Edit', { file_path: 'shared.txt', old_string: 'repeat', new_string: 'once' }, /not unique/],
        ['Edit', { file_path: 'shared.txt', old_string: 'repeat', new_string: 'once', replace_all: true }, /2 replacements/],
        ['Read', { file_path: '../escape.txt' }, /escape|root|traversal/],
        ['Grep', { pattern: '(once)\\s+\\1' }, /invalid or unsupported RE2 regex/],
        ['Grep', { pattern: 'a{1000}{1000}' }, /compilation complexity limit/],
        ['Grep', { pattern: '(ONCE|other)+', '-i': true, output_mode: 'count', glob: '*.txt' }, /shared.txt:1/],
      ],
      LEFT: [['Edit', { file_path: 'shared.txt', old_string: 'green', new_string: 'emerald' }, /Updated/]],
      RIGHT: [['Edit', { file_path: 'shared.txt', old_string: 'blue', new_string: 'azure' }, /Updated/]],
      OVERRIDE: [],
    };
    const server = createServer(async (request, response) => {
      try {
        const chunks = []; for await (const chunk of request) chunks.push(chunk);
        const body = JSON.parse(Buffer.concat(chunks)); trace.push(body);
        if (request.url === '/v1/responses') {
          const step = steps.get('CODEX') ?? 0;
          if (!codeMode) {
            const definitions = [...(body.tools ?? []), ...body.input.filter(item => item.type === 'additional_tools').flatMap(item => item.tools)];
            assert.ok(definitions.some(tool => tool.name === 'read_file'));
          }
          if (step > 0) {
            const result = body.input.findLast(item => item.type === 'function_call_output' || item.type === 'custom_tool_call_output');
            assert.ok(result, 'Codex received workspace read result');
            assert.match(JSON.stringify(result.output), /emerald/);
            assert.match(JSON.stringify(result.output), /azure/);
          }
          const output = step > 0
            ? [{ type: 'message', role: 'assistant', content: [{ type: 'output_text', text: 'CODEX_SHARED_DONE' }] }]
            : codeMode
              ? [{ type: 'custom_tool_call', call_id: 'codex-read', name: 'exec', input: 'text(await tools.read_file({path:"shared.txt"}));' }]
              : [{ type: 'function_call', call_id: 'codex-read', name: 'read_file', arguments: JSON.stringify({ path: 'shared.txt' }) }];
          steps.set('CODEX', step + 1);
          response.writeHead(200, { 'content-type': 'text/event-stream' });
          response.end(`data: ${JSON.stringify({ type: 'response.completed', response: { id: `codex-${step}`, status: 'completed', output, usage: { input_tokens: 10, output_tokens: 1, total_tokens: 11 } } })}\n\n`);
          return;
        }
        const label = Object.keys(scripts).find(key => JSON.stringify(body.messages[0]).includes(`WORKSPACE_${key}`));
        assert.ok(label, 'fixture identifies root prompt');
        const step = steps.get(label) ?? 0;
        const names = body.tools?.map(tool => tool.name) ?? [];
        if (!codeMode) {
          for (const name of ['Read', 'Edit', 'Write', 'Glob', 'Grep']) assert.equal(names.includes(name), label !== 'OVERRIDE', name);
        }
        const script = scripts[label];
        if (step > 0) {
          const results = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : []).filter(block => block.type === 'tool_result');
          const last = results.at(-1);
          assert.ok(last);
          const content = typeof last.content === 'string' ? last.content : JSON.stringify(last.content);
          // Code mode embeds text in a JSON envelope; decode its escapes for assertions.
          assert.match(content.replaceAll('\\t', '\t').replaceAll('\\n', '\n'), script[step - 1][2]);
        }
        let block;
        if (step === script.length) block = { type: 'text', text: `${label}_DONE` };
        else {
          const [name, input] = script[step];
          block = { type: 'tool_use', id: `${label}-${step}`, name, input };
          if (codeMode) block = { type: 'tool_use', id: `${label}-${step}`, name: 'exec', input: { code: `try { text(await tools.${name}(${JSON.stringify(input)})); } catch (error) { text(String(error)); }` } };
        }
        steps.set(label, step + 1);
        response.writeHead(200, { 'content-type': 'text/event-stream' }); response.end(stream(block));
      } catch (error) { errors.push(error.stack); response.destroy(error); }
    });
    await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
    t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
    const module = await readFile(new URL('../pkg-web/nanocodex_bg.wasm', import.meta.url));
    const transport = Transport.anthropic({ apiKey: 'synthetic', endpoint: `http://127.0.0.1:${server.address().port}/v1/messages` });
    let nano, override, failure;
    try {
      nano = await Nanocodex.create({ module, codeEvaluator, filesystem, workspace: '/project', model: 'claude-sonnet-4-6', backends: { claude: { transport, codeMode }, codex: { codeMode, transport: Transport.openAi({ apiKey: 'synthetic', apiBaseUrl: `http://127.0.0.1:${server.address().port}/v1`, stateless: true }) } } });
      const first = await nano.spawn(), second = await nano.spawn();
      assert.equal((await first.turn.prompt({ input: 'WORKSPACE_FIRST' }).result()).finalMessage, 'FIRST_DONE');
      assert.equal((await second.turn.prompt({ input: 'WORKSPACE_SECOND' }).result()).finalMessage, 'SECOND_DONE');
      const left = await nano.spawn(), right = await nano.spawn();
      const results = await Promise.all([left.turn.prompt({ input: 'WORKSPACE_LEFT' }).result(), right.turn.prompt({ input: 'WORKSPACE_RIGHT' }).result()]);
      assert.deepEqual(results.map(result => result.finalMessage), ['LEFT_DONE', 'RIGHT_DONE']);
      const codex = await nano.spawn({ harness: 'codex', model: 'gpt-6.1-sol' });
      assert.equal((await codex.turn.prompt({ input: 'Read the shared file edited by Claude.' }).result()).finalMessage, 'CODEX_SHARED_DONE');
      assert.equal(await readFile(join(directory, 'shared.txt'), 'utf8'), 'emerald\nazure\nonce once\n');
      assert.equal(await readFile(join(directory, 'nested/new.txt'), 'utf8'), 'new content\n');
      override = await Nanocodex.create({ module, codeEvaluator, filesystem, workspace: '/project', model: 'claude-sonnet-4-6', backends: { claude: { transport, codeMode, tools: [] } } });
      const explicit = await override.spawn();
      assert.equal((await explicit.turn.prompt({ input: 'WORKSPACE_OVERRIDE' }).result()).finalMessage, 'OVERRIDE_DONE');
      assert.deepEqual(errors, []);
    } catch (error) { failure = error; throw error; }
    finally {
      await override?.shutdown(); await nano?.shutdown();
      const output = new URL('../../../output/claude-workspace/', import.meta.url);
      await mkdir(output, { recursive: true });
      await writeFile(new URL(`journey-${codeMode ? 'code' : 'direct'}.json`, output), JSON.stringify({ status: failure ? 'failed' : 'passed', error: failure?.stack, trace, errors, finalFile: await readFile(join(directory, 'shared.txt'), 'utf8') }, null, 2));
    }
  });
}

// Node callers need only the runtime directory, without constructing an adapter.
import { Nanocodex as NodeNanocodex, Transport as NodeTransport } from '../node/index.mjs';

test('Node automatically opens the global workspace and rejects local overrides', { timeout: 30_000 }, async t => {
  const directory = await mkdtemp(join(tmpdir(), 'claude-node-workspace-'));
  t.after(() => rm(directory, { recursive: true, force: true }));
  await writeFile(join(directory, 'note.txt'), 'before\n');
  await mkdir(join(directory, 'node_modules'), { recursive: true });
  await writeFile(join(directory, 'node_modules', 'dependency.txt'), 'after dependency\n');
  // A .git file represents a linked worktree; it is excluded like a .git directory.
  await writeFile(join(directory, '.git'), 'after gitdir: fixture\n');
  await symlink(join(directory, 'note.txt'), join(directory, 'linked.txt'));
  const calls = [
    ['Read', { file_path: 'note.txt' }, /before/],
    ['Edit', { file_path: join(directory, 'note.txt'), old_string: 'before', new_string: 'after' }, /Updated note.txt/],
    ['Glob', { pattern: '**/*' }, /^note\.txt\n$/],
    ['Grep', { pattern: 'after' }, /^note\.txt\n$/],
    ['Grep', { pattern: 'after', path: 'node_modules' }, /^node_modules\/dependency\.txt\n$/],
  ];
  const trace = [], errors = [];
  const server = createServer(async (request, response) => {
    try {
      const chunks = []; for await (const chunk of request) chunks.push(chunk);
      const body = JSON.parse(Buffer.concat(chunks)); trace.push(body);
      assert.ok(body.tools.some(tool => tool.name === 'Read'));
      assert.ok(body.tools.some(tool => tool.name === 'Edit'));
      const step = trace.length - 1;
      if (step > 0) {
        const result = body.messages.flatMap(message => Array.isArray(message.content) ? message.content : []).filter(block => block.type === 'tool_result').at(-1);
        assert.equal(result.is_error ?? false, false);
        const content = typeof result.content === 'string' ? result.content : result.content.map(block => block.text ?? '').join('');
        assert.match(content, calls[step - 1][2]);
      }
      assert.ok(step <= calls.length);
      const block = step === calls.length ? { type: 'text', text: 'NODE_WORKSPACE_DONE' }
        : { type: 'tool_use', id: `node-${step}`, name: calls[step][0], input: calls[step][1] };
      response.writeHead(200, { 'content-type': 'text/event-stream' }); response.end(stream(block));
    } catch (error) { errors.push(error.stack); response.destroy(error); }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(async () => { server.closeAllConnections(); await new Promise(resolve => server.close(resolve)); });
  const transport = NodeTransport.anthropic({ apiKey: 'synthetic', endpoint: `http://127.0.0.1:${server.address().port}/v1/messages` });
  const options = { cua: false, workspace: directory, model: 'claude-sonnet-4-6', backends: { claude: { transport, codeMode: false } } };
  let nano, failure;
  try {
    await assert.rejects(NodeNanocodex.create({ ...options, backends: { claude: { ...options.backends.claude, workspace: directory } } }), /workspace belongs on Nanocodex.create/);
    nano = await NodeNanocodex.create(options);
    await assert.rejects(nano.spawn({ workspace: directory }), /unsupported .*spawn option: workspace/);
    assert.equal(trace.length, 0, 'invalid workspace overrides never reach provider');
    const agent = await nano.spawn();
    assert.equal((await agent.turn.prompt({ input: 'Read and update note.txt' }).result()).finalMessage, 'NODE_WORKSPACE_DONE');
    assert.equal(await readFile(join(directory, 'note.txt'), 'utf8'), 'after\n');
    assert.deepEqual(errors, []);
  } catch (error) { failure = error; throw error; }
  finally {
    await nano?.shutdown();
    const output = new URL('../../../output/claude-workspace/', import.meta.url);
    await mkdir(output, { recursive: true });
    await writeFile(new URL('journey-node-auto.json', output), JSON.stringify({ status: failure ? 'failed' : 'passed', error: failure?.stack, trace, errors, finalFile: await readFile(join(directory, 'note.txt'), 'utf8') }, null, 2));
  }
});
