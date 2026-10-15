#[path = "../src/mcp/search.rs"]
mod search;

use search::{SearchEngine, tokenize};

#[test]
fn tool_names_split_without_losing_unicode_or_acronyms() {
    assert_eq!(
        tokenize("GitHub_readHTTPFile 世界_Δοκιμή"),
        ["git", "hub", "read", "http", "file", "世界", "δοκιμή"]
    );
}

#[test]
fn search_ranks_specific_tools_and_preserves_query_term_frequency() {
    let engine = SearchEngine::new(
        [
            "github searchCode search code repositories",
            "filesystem readFile read file contents",
            "github createIssue create issue repository",
        ]
        .map(tokenize),
    );
    assert_eq!(engine.search(&tokenize("search code"), 1)[0].0, 0);
    assert_eq!(engine.search(&tokenize("read file"), 1)[0].0, 1);
    assert_eq!(engine.search(&tokenize("create issue"), 1)[0].0, 2);
    let once = engine.search(&tokenize("read"), 3);
    let twice = engine.search(&tokenize("read read"), 3);
    assert_eq!(twice[0].1, 2.0 * once[0].1);
    assert!(engine.search(&tokenize("missing"), 3).is_empty());
}

#[test]
fn empty_documents_limits_and_ties_are_deterministic() {
    let empty = SearchEngine::new([]);
    assert!(empty.search(&tokenize("anything"), 5).is_empty());
    let engine = SearchEngine::new([Vec::new(), Vec::new()]);
    assert!(engine.search(&tokenize("anything"), 5).is_empty());
    let engine = SearchEngine::new([tokenize("read file"), tokenize("read file")]);
    assert_eq!(
        engine
            .search(&tokenize("read"), 2)
            .iter()
            .map(|x| x.0)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(engine.search(&tokenize("read"), 1).len(), 1);
    assert!(engine.search(&tokenize("read"), 0).is_empty());
    assert!(engine.search(&[], 5).is_empty());
}
