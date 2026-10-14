import { resolveWorkspacePath } from './workspace.mjs';
import { RE2JS } from 're2js';

const MAX_FILE = 1024 * 1024;
const MAX_VISITS = 10_000;
const MAX_SCAN = 128 * MAX_FILE;
const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: true });
// Shared by every adapter for a given Workspace. The Workspace contract has no
// CAS primitive: writers outside these adapters still require host coordination.
const mutations = new WeakMap();
const string = { type: 'string' };
const natural = { type: 'integer', minimum: 0 };
const positive = { type: 'integer', minimum: 1 };

/** Narrow, browser-safe counterparts of ClaudeWorkspaceFiles' text tools.
 * Workspace backends own path isolation, parent creation and atomic writes.
 * No native processes, media decoders, or implicit permission interactions.
 */
export function claudeWorkspaceTools(workspace) {
  if (!workspace || typeof workspace.root !== 'string') throw new TypeError('workspace is required');
  for (const method of ['list', 'readFile', 'writeFile']) {
    if (typeof workspace[method] !== 'function') throw new TypeError(`workspace requires ${method}()`);
  }
  const path = (value, root = false) => {
    if (typeof value !== 'string' || encoder.encode(value).length > 4096) throw new Error('invalid workspace path');
    const relative = resolveWorkspacePath(workspace.root, value);
    if (!root && !relative) throw new Error('file path is empty');
    return relative;
  };
  const read = async file => {
    const bytes = await workspace.readFile(file);
    if (bytes.byteLength > MAX_FILE) throw new Error('file exceeds 1 MiB text limit');
    return decoder.decode(bytes);
  };
  const write = async (file, content) => {
    const bytes = encoder.encode(content);
    if (bytes.byteLength > MAX_FILE) throw new Error('file exceeds 1 MiB text limit');
    await workspace.writeFile(file, bytes);
  };
  const files = async scope => {
    const pending = [scope || '.'];
    const output = [];
    let visited = 0;
    while (pending.length) {
      const directory = pending.pop();
      const entries = await workspace.list(directory, { recursive: false, maxEntries: MAX_VISITS - visited + 1 });
      visited += entries.length;
      if (visited > MAX_VISITS) throw new Error('search exceeds 10000 entries');
      for (const entry of entries) {
        const relative = path(entry.path);
        const name = relative.split('/').at(-1);
        // Prune descendants only: callers may explicitly choose one of these
        // directories as their search root. Also omit git worktree link files.
        if (name === '.git' || name === 'node_modules') continue;
        if (entry.kind === 'directory') pending.push(relative);
        else if (entry.kind === 'file') output.push({ ...entry, path: relative });
      }
    }
    return output;
  };
  const tool = (name, description, properties, required, operation, mutation = false) => ({
    name, description, inputSchema: { type: 'object', properties, required, additionalProperties: false },
    supportsParallelToolCalls: !mutation,
    async handler(input) {
      validate(input, properties, required, name);
      if (!mutation) return operation(input);
      const previous = mutations.get(workspace) ?? Promise.resolve();
      const result = previous.catch(() => {}).then(() => operation(input));
      mutations.set(workspace, result);
      try { return await result; }
      finally { if (mutations.get(workspace) === result) mutations.delete(workspace); }
    },
  });
  return [
    tool('Read', 'Read UTF-8 text with numbered lines (one-based offset). Maximum file size 1 MiB. Media and PDF pages are unsupported.',
      { file_path: string, offset: positive, limit: positive, pages: string }, ['file_path'], async input => {
        if (input.pages !== undefined) throw new Error('PDF pages are unsupported by this workspace adapter');
        const lines = textLines(await read(path(input.file_path)));
        const start = (input.offset ?? 1) - 1;
        return lines.slice(start, start + (input.limit ?? lines.length)).map((line, i) => `${start + i + 1}\t${line}\n`).join('');
      }),
    tool('Edit', 'Replace exact text, requiring one occurrence unless replace_all is true. Maximum file size 1 MiB. Mutations sharing this Workspace are serialized.',
      { file_path: string, old_string: string, new_string: string, replace_all: { type: 'boolean' } }, ['file_path', 'old_string', 'new_string'], async input => {
        if (!input.old_string) throw new Error('old_string must not be empty');
        if (input.old_string === input.new_string) throw new Error('old_string and new_string must differ');
        const file = path(input.file_path);
        const before = await read(file);
        const parts = before.split(input.old_string);
        const count = parts.length - 1;
        if (!count) throw new Error('old_string not found');
        if (count !== 1 && !input.replace_all) throw new Error('old_string is not unique; use replace_all or provide more context');
        const outputBytes = encoder.encode(before).length + count * (encoder.encode(input.new_string).length - encoder.encode(input.old_string).length);
        if (outputBytes > MAX_FILE) throw new Error('file exceeds 1 MiB text limit');
        const after = parts.join(input.new_string);
        // Catch changes made while awaiting a custom backend. This is detection,
        // not an atomic compare-and-swap against unrelated external writers.
        if (await read(file) !== before) throw new Error('file changed during Edit; read it again');
        await write(file, after);
        return `Updated ${file} (${count} replacements)\n`;
      }, true),
    tool('Write', 'Replace a UTF-8 workspace file, creating parents through the workspace backend. Maximum file size 1 MiB.',
      { file_path: string, content: string }, ['file_path', 'content'], async input => {
        const file = path(input.file_path);
        await write(file, input.content);
        return `Wrote ${file}\n`;
      }, true),
    tool('Glob', 'List workspace files newest first (path order when timestamps are absent). Supports *, ?, and ** as a whole path component; braces, classes and escapes are unsupported. Maximum 10000 entries; skips descendant .git and node_modules entries (explicit search roots are allowed).',
      { pattern: string, path: string }, ['pattern'], async input => {
        const scope = path(input.path ?? '.', true);
        const pattern = input.pattern.startsWith('/') ? path(input.pattern) : input.pattern;
        const match = globMatcher(pattern);
        const entries = await files(scope);
        entries.sort((a, b) => (b.modifiedAt ?? 0) - (a.modifiedAt ?? 0) || a.path.localeCompare(b.path));
        return entries.filter(entry => match(entry.path.slice(scope ? scope.length + 1 : 0))).map(entry => `${entry.path}\n`).join('');
      }),
    tool('Grep', 'Search UTF-8 text with RE2 regular expressions, including groups, alternation and quantifiers. Backreferences and lookaround are unsupported. Patterns <=4096 bytes with bounded compilation complexity. Supports directory or file path, glob (*, ?, **; leading ! excludes), -i, -n, output_mode, head_limit and offset. No multiline, context or file-type filters. Files <=1 MiB; scan <=128 MiB and 10000 entries. Skips descendant .git and node_modules entries (explicit search roots are allowed).',
      { pattern: string, path: string, glob: string, '-i': { type: 'boolean' }, '-n': { type: 'boolean', default: true }, output_mode: { type: 'string', enum: ['content', 'files_with_matches', 'count'], default: 'files_with_matches' }, head_limit: { ...natural, default: 250 }, offset: { ...natural, default: 0 } }, ['pattern'], async input => {
        const regex = boundedRegex(input.pattern, input['-i']);
        const scope = path(input.path ?? '.', true);
        let entries;
        if (scope) {
          const parent = scope.split('/').slice(0, -1).join('/');
          const siblings = await workspace.list(parent || '.', { maxEntries: MAX_VISITS + 1 });
          if (siblings.length > MAX_VISITS) throw new Error('search exceeds 10000 entries');
          const entry = siblings.find(item => path(item.path) === scope);
          if (!entry) throw new Error('search path does not exist');
          entries = entry.kind === 'file' ? [{ ...entry, path: scope }] : await files(scope);
        } else entries = await files(scope);
        entries.sort((a, b) => a.path.localeCompare(b.path));
        const exclude = input.glob?.startsWith('!');
        const glob = input.glob === undefined ? null : globMatcher(exclude ? input.glob.slice(1) : input.glob);
        const mode = input.output_mode ?? 'files_with_matches';
        const rows = [];
        let scanned = 0, skipped = 0;
        const limit = input.head_limit ?? 250;
        const add = row => {
          if (skipped++ < (input.offset ?? 0)) return;
          rows.push(row);
        };
        for (const entry of entries) {
          const local = scope && entry.path !== scope ? entry.path.slice(scope.length + 1) : entry.path;
          if (glob && (glob(local) || (!input.glob.includes('/') && glob(entry.path.split('/').at(-1)))) === !!exclude) continue;
          if (entry.size > MAX_FILE) continue;
          const bytes = await workspace.readFile(entry.path);
          scanned += bytes.byteLength;
          if (scanned > MAX_SCAN) throw new Error('search exceeds 128 MiB scan limit');
          if (bytes.byteLength > MAX_FILE) continue;
          let content;
          try { content = decoder.decode(bytes); } catch { continue; }
          if (content.includes('\0')) continue;
          const lines = textLines(content);
          let count = 0;
          for (let i = 0; i < lines.length; i++) {
            if (!regex.test(lines[i])) continue;
            count++;
            if (mode === 'content') {
              add(`${entry.path}:${input['-n'] === false ? '' : `${i + 1}:`}${lines[i]}\n`);
              if (limit && rows.length >= limit) return rows.join('');
            }
          }
          if (count && mode !== 'content') add(`${entry.path}${mode === 'count' ? `:${count}` : ''}\n`);
          if (limit && rows.length >= limit) break;
        }
        return rows.join('');
      }),
  ];
}

function textLines(text) {
  if (!text) return [];
  const lines = text.split('\n');
  if (lines.at(-1) === '') lines.pop();
  return lines.map(line => line.endsWith('\r') ? line.slice(0, -1) : line);
}
function validate(input, properties, required, name) {
  if (!input || typeof input !== 'object' || Array.isArray(input)) throw new Error(`${name} input must be an object`);
  for (const key of required) if (!(key in input)) throw new Error(`missing ${key}`);
  for (const [key, value] of Object.entries(input)) {
    const schema = properties[key];
    if (!Object.hasOwn(properties, key)) throw new Error(`unsupported ${name} option: ${key}`);
    if (schema.type === 'integer' ? !Number.isSafeInteger(value) || value < schema.minimum : typeof value !== schema.type) throw new Error(`invalid ${key}`);
    if (schema.enum && !schema.enum.includes(value)) throw new Error(`invalid ${key}`);
  }
}
function boundedRegex(pattern, insensitive) {
  if (!pattern || encoder.encode(pattern).length > 4096) throw new Error('pattern must be 1 to 4096 bytes');
  // Admit counted repetition before RE2 allocates its expanded program. This
  // conservative budget follows the existing shell-search admission approach;
  // RE2 itself prevents exponential backtracking during matching.
  let expansion = 1;
  for (const match of pattern.matchAll(/\{(\d+)(?:,(\d*))?\}/g)) {
    expansion *= Math.max(1, Number(match[2] || match[1]));
    if (!Number.isSafeInteger(expansion) || expansion * pattern.length > 65_536) {
      throw new Error('regex exceeds compilation complexity limit');
    }
  }
  let regex;
  try { regex = RE2JS.compile(pattern, insensitive ? RE2JS.CASE_INSENSITIVE : 0); }
  catch (error) { throw new Error(`invalid or unsupported RE2 regex: ${error.message}`); }
  if (regex.programSize() > 16_384) throw new Error('regex exceeds compiled program limit');
  return regex;
}
// Component-level DP keeps glob matching polynomial even with many stars.
function globMatcher(pattern) {
  if (pattern.startsWith('./')) pattern = pattern.slice(2);
  if (!pattern || pattern.length > 512 || pattern.startsWith('/') || /[\[\]{}\\]/.test(pattern)) throw new Error('unsupported glob pattern');
  const parts = pattern.split('/');
  if (parts.some(part => !part || part === '.' || part === '..' || (part.includes('**') && part !== '**'))) throw new Error('invalid glob pattern');
  const wildcard = (pattern, text) => {
    let row = Array(text.length + 1).fill(false); row[0] = true;
    for (const char of pattern) {
      const next = Array(text.length + 1).fill(false);
      next[0] = char === '*' && row[0];
      for (let i = 1; i <= text.length; i++) next[i] = char === '*' ? row[i] || next[i - 1] : row[i - 1] && (char === '?' || char === text[i - 1]);
      row = next;
    }
    return row[text.length];
  };
  return text => {
    const names = text.split('/');
    let row = Array(names.length + 1).fill(false); row[0] = true;
    for (const part of parts) {
      const next = Array(names.length + 1).fill(false); next[0] = part === '**' && row[0];
      for (let i = 1; i <= names.length; i++) next[i] = part === '**' ? row[i] || next[i - 1] : row[i - 1] && wildcard(part, names[i - 1]);
      row = next;
    }
    return row[names.length];
  };
}
