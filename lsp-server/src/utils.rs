//! Shared utility functions

/// Convert a camelCase or PascalCase identifier to snake_case
///
/// Examples:
/// - `getUserProfile` → `get_user_profile`
/// - `createUser` → `create_user`
/// - `ping` → `ping`
#[must_use]
pub fn camel_to_snake(s: &str) -> String {
    let mut result = String::with_capacity(s.len() + 4);
    let chars: Vec<char> = s.chars().collect();

    for (i, &ch) in chars.iter().enumerate() {
        if ch.is_ascii_uppercase() {
            // Insert underscore before uppercase if:
            // - not at position 0
            // - previous char is lowercase OR (next char is lowercase AND previous is uppercase)
            let prev_is_lower = i > 0 && chars[i - 1].is_ascii_lowercase();
            let next_is_lower = chars.get(i + 1).is_some_and(|c| c.is_ascii_lowercase());
            let prev_is_upper = i > 0 && chars[i - 1].is_ascii_uppercase();

            if prev_is_lower || (next_is_lower && prev_is_upper) {
                result.push('_');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_camel_to_snake_basic() {
        assert_eq!(camel_to_snake("getUserProfile"), "get_user_profile");
        assert_eq!(camel_to_snake("createUser"), "create_user");
        assert_eq!(camel_to_snake("ping"), "ping");
    }

    #[test]
    fn test_camel_to_snake_already_snake() {
        assert_eq!(camel_to_snake("get_user"), "get_user");
    }

    #[test]
    fn test_camel_to_snake_single_word() {
        assert_eq!(camel_to_snake("ping"), "ping");
        assert_eq!(camel_to_snake("Ping"), "ping");
    }

    #[test]
    fn test_camel_to_snake_acronym() {
        // "getHTTPSResponse" -> "get_https_response"
        assert_eq!(camel_to_snake("getHTTPSResponse"), "get_https_response");
    }
}
