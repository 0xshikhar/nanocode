# Schema validation

`nanocode-schema` compiles JSON Schema drafts 4, 6, 7, 2019-09 and 2020-12 for
subagent output contracts. Schemas without `$schema` use draft 2020-12. Legacy
drafts assert formats; modern drafts follow the format vocabulary. Schema
compilation validates against the embedded metaschemas. Serialization continues
to use serde_json's default sorted object keys.

The engine derives from [Boon 0.6.1](https://crates.io/crates/boon/0.6.1), by
Santhosh Kumar Tekuri, under MIT OR Apache-2.0. Both upstream licenses are retained
in this directory. This is a locally maintained adaptation, rather than a new
schema specification implementation. Changes from upstream:

- Standard-library HashMap, DefaultHasher and LazyLock replace ahash and once_cell.
- ECMAScript patterns use regress with Unicode semantics, including lookaround and backreferences.
- The resource loader performs no default filesystem or network access on either target; compiler locations require absolute URIs.
- The output-contract facade preserves validation and instance/schema error paths, with a boolean validation path for early failure.
- Numeric bounds preserve integer precision; decimal divisibility uses bounded serde_json numbers without a big-integer dependency.
- Length, item and property limits retain their numeric values rather than narrowing to the target pointer width.
- Sources follow the workspace's Rust edition, lint and comment conventions.

`Compiler::add_resource` registers documents in memory. Hosts can explicitly
supply a `UrlLoader` when external references are needed. Output contracts use
only in-memory resources and embedded standard metaschemas. The crate compiles
without JavaScript bindings or a host random-number import.

Maintain the engine against the official
[JSON Schema Test Suite](https://github.com/json-schema-org/JSON-Schema-Test-Suite).
The dependency migration's pinned-suite results and before/after benchmarks are
recorded under ignored `output/dependency-reduction/`.

Format compatibility helpers in `src/format_compat.rs` derive from
[jsonschema 0.48.5](https://crates.io/crates/jsonschema/0.48.5), copyright
2020–2026 Dmitry Dygalo, under its MIT license (`LICENSE-jsonschema`). These
preserve the existing email, IDNA, URI-template and duration rules. The small
email_address, unicode-general-category and num-cmp dependencies remain for
protocol-specific parsing, Unicode data and exact integer/float comparison.
`numbers.rs` handles serde_json's bounded i64/u64/f64 representation; its decimal
coefficients fit u128 and IEEE-754 decimal exponents fit i32.

Run the contract regressions with `cargo test -p nanocode-schema --test contracts`.
To run the official required suite, check out revision
`55719286c33ae0d1823bfb65fea10338a19bc37c` and run:

```sh
JSON_SCHEMA_TEST_SUITE=/absolute/path/to/JSON-Schema-Test-Suite cargo test -p nanocode-schema --test conformance -- --ignored --nocapture
```

The migration also compared all optional cases against the previous validator.
Optional conformance is not complete; the migration introduces no regressions
on cases that the previous validator passed. Required cases pass for all five
drafts, through both boolean and detailed validation paths.
