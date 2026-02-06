//! Single File Component (SFC) handling for Vue, Svelte, etc.

/// Extract ALL script blocks from SFC (Single File Component: Vue, Svelte, etc.)
/// Returns tuples of (`script_content`, `line_offset`) for each <script> block found
pub fn extract_script_blocks(content: &str) -> Vec<(String, usize)> {
    let mut blocks = Vec::new();
    let mut search_pos = 0;

    while let Some(tag_start) = content[search_pos..].find("<script") {
        let absolute_tag_start = search_pos + tag_start;

        // Find end of opening tag (>)
        let Some(tag_close_offset) = content[absolute_tag_start..].find('>') else {
            break;
        };

        let tag_close = absolute_tag_start + tag_close_offset + 1;

        // Find closing </script>
        let Some(end_tag_offset) = content[tag_close..].find("</script>") else {
            break;
        };

        let script_end = tag_close + end_tag_offset;

        // Extract script content
        let script_content = &content[tag_close..script_end];

        // Calculate line offset
        let line_offset = content[..tag_close].lines().count().saturating_sub(1);

        blocks.push((script_content.to_string(), line_offset));

        // Move search position past this script block
        search_pos = script_end + "</script>".len();
    }

    blocks
}
