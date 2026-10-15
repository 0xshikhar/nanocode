# Codex upstream contracts

- Preserve the exact prompt bytes and extraction provenance in `prompts/upstream.json`.
- Keep the immutable provider prefix on `ModelConfig::base_system_prompt`; place runtime identity and additional instructions in developer context. Preserve the public composed `system_prompt()` compatibility view and explicit system overrides.
- `upstream/tools.json` pins the reviewed standard tool sources, local artifacts, and selected upstream option profile. Re-audit affected contracts against that revision before updating artifact hashes.
- Run `python3 scripts/verify-codex-upstream.py` from the repository root to verify upstream hashes, prompt extraction, local artifact integrity, and exact patch grammar. `--offline` uses the ignored `output/codex-upstream-parity` cache. Without `--serialize`, this verifies artifacts without executing Rust tool serialization.
- Add `--serialize` to execute the pinned upstream standard tool builders in an ignored minimal consumer binary. `--consumer-dump PATH` compares its five serialized contracts against a public SDK consumer JSON dump containing `codex_standard`. The harness preserves upstream handler bodies, schema types and selected wire serializers while omitting unrelated host dependencies.
- Keep output schemas as host metadata, matching upstream omission from Responses tool serialization.
- Local `web.run`, `image_gen.imagegen`, attachment, and delegation functions are SDK extension contracts. Do not describe them as upstream provider server tool parity. Host routing and shared Code Mode can decorate or transform model-visible exposure independently of the pinned standard definitions.
