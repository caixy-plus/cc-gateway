use crate::i18n::lang::{first_apple_language, Language};

use super::helpers::TestEnv;

#[test]
fn detect_skips_neutral_lc_all_and_uses_chinese_lang() {
    let _env = TestEnv::new();
    std::env::remove_var("CC_GATEWAY_LANG");
    std::env::set_var("LC_ALL", "C");
    std::env::set_var("LANG", "zh_CN.UTF-8");

    assert_eq!(Language::detect(), Language::ZhCN);
}

#[test]
fn windows_prefers_system_locale_over_git_lang_env() {
    assert_eq!(
        Language::detect_from(
            None,
            None,
            Some("en_US.UTF-8"),
            Some("zh-CN"),
            true,
        ),
        Language::ZhCN,
    );
}

#[test]
fn unix_prefers_lang_over_system_locale() {
    assert_eq!(
        Language::detect_from(
            None,
            None,
            Some("zh_CN.UTF-8"),
            Some("en-US"),
            false,
        ),
        Language::ZhCN,
    );
}

#[test]
fn parses_first_macos_apple_language() {
    let output = r#"(
    "zh-Hans-CN",
    "en-CN"
)"#;

    assert_eq!(first_apple_language(output).as_deref(), Some("zh-Hans-CN"));
}
