pub const BGE_DOCUMENT_PREFIX: &str = "Represent this sentence: ";
pub const BGE_QUERY_PREFIX: &str = "Represent this question for searching relevant passages: ";

pub fn prefix_document_for_index(text: &str) -> String {
    prefix_once(text, BGE_DOCUMENT_PREFIX)
}

pub fn prefix_query_for_search(text: &str) -> String {
    prefix_once(text, BGE_QUERY_PREFIX)
}

fn prefix_once(text: &str, prefix: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with(prefix) {
        trimmed.to_string()
    } else {
        format!("{prefix}{trimmed}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bge_document_and_query_prefixes_are_distinct_and_stable() {
        let text = "memory vault graph work";

        assert_eq!(
            prefix_document_for_index(text),
            "Represent this sentence: memory vault graph work"
        );
        assert_eq!(
            prefix_query_for_search(text),
            "Represent this question for searching relevant passages: memory vault graph work"
        );
        assert_ne!(
            prefix_document_for_index(text),
            prefix_query_for_search(text)
        );
    }

    #[test]
    fn bge_prefix_helpers_do_not_double_prefix() {
        let prefixed = "Represent this sentence: memory vault graph work";
        assert_eq!(prefix_document_for_index(prefixed), prefixed);

        let query_prefixed =
            "Represent this question for searching relevant passages: memory vault graph work";
        assert_eq!(prefix_query_for_search(query_prefixed), query_prefixed);
    }
}
