import argparse
import concurrent.futures
import hashlib
import json
import os
from pathlib import Path
import subprocess
import urllib.request

root = Path(__file__).resolve().parents[1]
backend = root / 'crates/backends/codex'
parser = argparse.ArgumentParser()
parser.add_argument('--offline', action='store_true')
parser.add_argument('--serialize', action='store_true')
parser.add_argument('--consumer-dump', type=Path)
args = parser.parse_args()
output = root / 'output/codex-upstream-parity'
output.mkdir(parents=True, exist_ok=True)
prompts = json.loads((backend / 'prompts/upstream.json').read_text())
tools = json.loads((backend / 'upstream/tools.json').read_text())
if prompts['revision'] != tools['revision']:
    raise ValueError('Prompt and tool provenance revisions differ')


def digest(data):
    return hashlib.sha256(data).hexdigest()


def require_hash(data, expected, label):
    actual = digest(data)
    if actual != expected:
        raise ValueError(f'{label}: expected {expected}, received {actual}')


def source(entry):
    cached = output / entry['path'].replace('/', '__')
    if args.offline:
        data = cached.read_bytes()
    else:
        url = f"https://raw.githubusercontent.com/openai/codex/{tools['revision']}/{entry['path']}"
        data = urllib.request.urlopen(url).read()
        cached.write_bytes(data)
    require_hash(data, entry['sha256'], entry['path'])
    return entry['path'], data


with concurrent.futures.ThreadPoolExecutor() as executor:
    sources = dict(executor.map(source, tools['sources']))
models_data = sources['codex-rs/models-manager/models.json']
require_hash(models_data, prompts['source_sha256'], 'model catalog')
models = json.loads(models_data)
for entry in prompts['prompts']:
    value = models
    for part in entry['json_pointer'].strip('/').split('/'):
        value = value[int(part)] if isinstance(value, list) else value[part]
    extracted = value.encode('utf-8')
    local = (backend / 'prompts' / entry['file']).read_bytes()
    require_hash(local, entry['sha256'], entry['file'])
    if local != extracted or len(local) != entry['bytes']:
        raise ValueError(f"Prompt extraction differs: {entry['file']}")
for entry in tools['local_artifacts']:
    require_hash((backend / entry['path']).read_bytes(), entry['sha256'], entry['path'])
grammar = (backend / 'src/builtin_tools/apply_patch/apply_patch.lark').read_bytes()
if grammar != sources['codex-rs/core/assets/tools/apply_patch.lark']:
    raise ValueError('apply_patch grammar differs from pinned upstream')
report = {
    'revision': tools['revision'],
    'prompt_bytes_verified': [entry['file'] for entry in prompts['prompts']],
    'source_hashes_verified': list(sources),
    'local_artifact_hashes_verified': [entry['path'] for entry in tools['local_artifacts']],
    'grammar_bytes_verified': True,
    'profile': tools['profile'],
    'scope': tools['scope'],
    'method': 'Prompt extraction and grammar compare bytes. Tool sources and reviewed local implementations are hash locked. --serialize additionally executes the pinned upstream builders; host behavior equivalence is outside this check.',
}
if args.serialize:
    probe = output / 'serializer'
    probe.mkdir(exist_ok=True)
    (probe / 'Cargo.toml').write_text(
        '[package]\nname = "codex-upstream-serializer"\nversion = "0.0.0"\n'
        'edition = "2024"\n[workspace]\n[dependencies]\n'
        'serde = { version = "1", features = ["derive"] }\nserde_json = "1"\n'
        '[[bin]]\nname = "codex-upstream-serializer"\npath = "main.rs"\n'
    )
    api_source = sources['codex-rs/tools/src/responses_api.rs'].decode()
    items = []
    for name in ['FreeformTool', 'FreeformToolFormat', 'ResponsesApiTool']:
        start = api_source.index(f'pub struct {name} {{')
        start = api_source.rfind('#[derive(', 0, start)
        end = api_source.index('\n}', start) + 2
        items.append(api_source[start:end])
    modules = []
    for module, path in [
        ('schema', 'codex-rs/tools/src/json_schema/types.rs'),
        ('shell', 'codex-rs/core/src/tools/handlers/shell_spec.rs'),
        ('plan', 'codex-rs/core/src/tools/handlers/plan_spec.rs'),
        ('view', 'codex-rs/core/src/tools/handlers/view_image_spec.rs'),
    ]:
        modules.append(f'#[path = {json.dumps(str(output / path.replace("/", "__")))}] mod {module};')
    patch = sources['codex-rs/core/src/tools/handlers/apply_patch_spec.rs'].decode()
    patch = patch[:patch.index('\n#[cfg(test)]')]
    patch = patch.replace('"../../../assets/tools/apply_patch.lark"', json.dumps(
        str(output / 'codex-rs__core__assets__tools__apply_patch.lark')))
    (probe / 'patch.rs').write_text(patch)
    code = (
        'extern crate self as codex_tools;\nextern crate self as codex_protocol;\n'
        'use serde::{Serialize, Deserialize};\npub use schema::JsonSchema;\n'
        'type ToolOutputSchema = serde_json::Value;\n'
        'pub mod models { pub const VIEW_IMAGE_TOOL_NAME: &str = "view_image"; }\n'
        + '\n'.join(items)
        + '\n#[derive(Serialize)]\n#[serde(tag = "type")]\n'
        'pub enum ToolSpec { #[serde(rename = "function")] Function(ResponsesApiTool), '
        '#[serde(rename = "custom")] Freeform(FreeformTool) }\n'
        + '\n'.join(modules) + '\nmod patch;\n'
        'fn main() {\nlet tools = ['
        'shell::create_exec_command_tool_with_environment_id('
        'shell::CommandToolOptions {exec_permission_approvals_enabled:false},false,false),'
        'shell::create_write_stdin_tool(),plan::create_update_plan_tool(),'
        'patch::create_apply_patch_freeform_tool(false),'
        'view::create_view_image_tool(view::ViewImageToolOptions {'
        'can_request_original_image_detail:true,unified_image_budget:false,include_environment_id:false})];\n'
        'println!("{}",serde_json::to_string_pretty(&tools).unwrap());\n}\n'
    )
    (probe / 'main.rs').write_text(code)
    with (output / 'serializer-build.log').open('w') as log:
        result = subprocess.run(
            ['cargo', 'run', '--quiet', '--offline', '--manifest-path', str(probe / 'Cargo.toml')],
            cwd=root, env={**os.environ, 'CARGO_TARGET_DIR': str(root / 'target')},
            stdout=subprocess.PIPE, stderr=log, check=True,
        )
    serialized = json.loads(result.stdout)
    (output / 'upstream-tools.json').write_text(json.dumps(serialized, indent=2) + '\n')
    report['upstream_builders_executed'] = [tool['name'] for tool in serialized]
    report['serializer_scope'] = (
        'Pinned upstream handler bodies and JsonSchema types execute unchanged. '
        'The harness extracts the upstream ResponsesApiTool/FreeformTool structs and uses '
        'their two selected ToolSpec serde variants; unused namespace/server-tool variants '
        'and unrelated host dependencies are omitted.'
    )
    if args.consumer_dump:
        consumer = json.loads(args.consumer_dump.read_text())['codex_standard']
        if {tool['name']: tool for tool in consumer} != {tool['name']: tool for tool in serialized}:
            raise ValueError('SDK standard tool serialization differs from upstream builders')
        report['consumer_serialization_exact'] = [tool['name'] for tool in serialized]
elif args.consumer_dump:
    raise ValueError('--consumer-dump requires --serialize')
(output / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
