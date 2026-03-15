use lsp_server::utils::lsp_character_to_byte_index;

#[test]
fn test_lsp_character_to_byte_index_basic() {
    let s = "aбc";
    assert_eq!(lsp_character_to_byte_index(s, 0), 0); // before 'a'
    assert_eq!(lsp_character_to_byte_index(s, 1), 1); // before 'б'
    assert_eq!(lsp_character_to_byte_index(s, 2), 3); // before 'c' (1+2 bytes)
    assert_eq!(lsp_character_to_byte_index(s, 3), 4); // end
}

#[test]
fn test_lsp_character_to_byte_index_supplemental() {
    // 𐐀 is U+10400. In UTF-16 it is D801 DC00 (2 code units). In UTF-8 it is F0 90 90 80 (4 bytes).
    let s2 = "a𐐀c";
    assert_eq!(lsp_character_to_byte_index(s2, 0), 0);
    assert_eq!(lsp_character_to_byte_index(s2, 1), 1); // before 𐐀
    assert_eq!(lsp_character_to_byte_index(s2, 3), 5); // before 'c' (1+4 bytes). Note index 2 is inside 𐐀 in utf16 terms
    assert_eq!(lsp_character_to_byte_index(s2, 4), 6);
}

#[test]
fn test_lsp_character_to_byte_index_chinese() {
    // Chinese characters are usually 3 bytes in UTF-8 and 1 unit in UTF-16 (BMP).
    // 你好 (Nǐ hǎo) - Hello
    // 你: U+4F60. UTF-8: E4 BD A0 (3 bytes). UTF-16: 4F60 (1 unit).
    // 好: U+597D. UTF-8: E5 A5 BD (3 bytes). UTF-16: 597D (1 unit).

    let user_msg = "test你好";
    // "test" (4 bytes, 4 chars)
    // "你好" (6 bytes, 2 chars)

    assert_eq!(lsp_character_to_byte_index(user_msg, 4), 4); // before '你'
    assert_eq!(lsp_character_to_byte_index(user_msg, 5), 7); // before '好' (4 + 3)
    assert_eq!(lsp_character_to_byte_index(user_msg, 6), 10); // end (4 + 3 + 3)
}

#[test]
fn test_lsp_character_to_byte_index_arabic() {
    // Arabic characters are also in BMP, 2 bytes in UTF-8 usually? No, mostly 2 bytes.
    // Let's check: 'م' (Meem) U+0645. UTF-8: D9 85 (2 bytes).
    // 'ر' (Reh) U+0631. UTF-8: D8 B1 (2 bytes).
    // مرحبا (Marhaban)

    let s = "مرحبا";
    // م (2), ر (2), ح (2), ب (2), ا (2) = 10 bytes total. 5 chars.

    assert_eq!(lsp_character_to_byte_index(s, 0), 0);
    assert_eq!(lsp_character_to_byte_index(s, 1), 2);
    assert_eq!(lsp_character_to_byte_index(s, 5), 10);
}
