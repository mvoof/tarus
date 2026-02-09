use lsp_server::tree_parser::query_helpers::CaptureIndices;
use tree_sitter::Query;

#[test]
fn test_capture_indices_creation() {
    // Create a simple test query
    let query_str = r"
        (function_item
            name: (identifier) @func_name
            parameters: (parameters) @func_params)
    ";

    let language: tree_sitter::Language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&language, query_str).unwrap();

    let indices = CaptureIndices::from_query(&query, &["func_name", "func_params"]);

    assert!(indices.get("func_name").is_some());
    assert!(indices.get("func_params").is_some());
    assert!(indices.get("nonexistent").is_none());
}
