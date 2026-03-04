use lsp_server::utils::{camel_to_kebab, camel_to_snake};

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
    assert_eq!(camel_to_snake("getHTTPSResponse"), "get_https_response");
}

#[test]
fn test_camel_to_kebab_basic() {
    assert_eq!(camel_to_kebab("globalEvent"), "global-event");
    assert_eq!(camel_to_kebab("myCustomEvent"), "my-custom-event");
    assert_eq!(camel_to_kebab("userProfileUpdated"), "user-profile-updated");
    assert_eq!(camel_to_kebab("ping"), "ping");
}

#[test]
fn test_camel_to_kebab_single_word() {
    assert_eq!(camel_to_kebab("event"), "event");
    assert_eq!(camel_to_kebab("Event"), "event");
}

#[test]
fn test_camel_to_kebab_acronym() {
    assert_eq!(camel_to_kebab("getHTTPSResponse"), "get-https-response");
}
