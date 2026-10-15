# Tool search

`search.rs` implements the immutable BM25 index used by the MCP catalog.
Tokenization preserves the catalog's Unicode alphanumeric and camel-case
splitting. The scoring formula and parameters match bm25 2.3.2's defaults:
`k1 = 1.2`, `b = 0.75`, document-length normalization and smoothed inverse
document frequency `ln(1 + (N - df + 0.5) / (df + 0.5))`.

The index stores postings by token text, avoiding the old crate's 32-bit token
hash collisions and redundant postings for repeated document tokens. Repeated
query terms still contribute repeatedly to the score. Equal scores sort by
catalog document index for deterministic results. The caller supplies the result
limit; there is no separate index-size or query-size limit.
