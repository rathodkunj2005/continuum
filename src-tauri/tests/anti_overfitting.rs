//! Guards against fixture-specific hardcoded synonym mappings in production retrieval logic.

use std::fs;

#[test]
fn production_query_and_intent_logic_avoids_fixture_specific_alias_rules() {
    let checks = [
        (
            "src/search/query_processor.rs",
            vec!["\"ipl\"", "\"cricket\"", "\"soccer\"", "\"football\""],
        ),
        (
            "src/context_runtime/mod.rs",
            vec!["contains(\"fndr\")", "Some(\"FNDR\".to_string())"],
        ),
        (
            "src/memory_embedding_document.rs",
            vec!["\"ipl\"", "\"cricket\"", "\"soccer\"", "\"football\""],
        ),
    ];

    for (relative_path, banned_terms) in checks {
        let path = format!("{}/{}", env!("CARGO_MANIFEST_DIR"), relative_path);
        let source = fs::read_to_string(&path).expect("read source file");
        for term in banned_terms {
            assert!(
                !source.contains(term),
                "found fixture-specific mapping '{term}' in {relative_path}"
            );
        }
    }
}
