use crate::i18n::lang::Language;
use crate::i18n::{current_language, init};
use std::sync::Mutex;

// i18n tests mutate global state, so serialize them.
static I18N_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn test_language_from_str_zh() {
    assert_eq!(Language::from_str("zh_CN"), Language::ZhCN);
    assert_eq!(Language::from_str("zh-CN"), Language::ZhCN);
    assert_eq!(Language::from_str("zh"), Language::ZhCN);
    assert_eq!(Language::from_str("ZH"), Language::ZhCN);
}

#[test]
fn test_language_from_str_en() {
    assert_eq!(Language::from_str("en"), Language::En);
    assert_eq!(Language::from_str("EN"), Language::En);
    assert_eq!(Language::from_str("en_US"), Language::En);
    assert_eq!(Language::from_str("fr"), Language::En);
}

#[test]
fn test_language_detect_with_cc_gateway_lang() {
    let _guard = I18N_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("CC_GATEWAY_LANG", "zh_CN");
    assert_eq!(Language::detect(), Language::ZhCN);

    std::env::set_var("CC_GATEWAY_LANG", "en");
    assert_eq!(Language::detect(), Language::En);

    std::env::remove_var("CC_GATEWAY_LANG");
}

#[test]
fn test_init_sets_current_language() {
    let _guard = I18N_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("CC_GATEWAY_LANG", "zh_CN");
    init();
    assert_eq!(current_language(), Language::ZhCN);

    std::env::set_var("CC_GATEWAY_LANG", "en");
    init();
    assert_eq!(current_language(), Language::En);

    std::env::remove_var("CC_GATEWAY_LANG");
}

#[test]
fn test_translation_lookup() {
    let _guard = I18N_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("CC_GATEWAY_LANG", "en");
    init();
    let text = crate::i18n::dict::t("daemon.started");
    assert!(text.contains("daemon started"));

    std::env::set_var("CC_GATEWAY_LANG", "zh_CN");
    init();
    let text = crate::i18n::dict::t("daemon.started");
    assert!(text.contains("守护进程已启动"));

    std::env::remove_var("CC_GATEWAY_LANG");
}

#[test]
fn test_translation_missing_key_returns_key() {
    let _guard = I18N_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    std::env::set_var("CC_GATEWAY_LANG", "en");
    init();
    assert_eq!(crate::i18n::dict::t("nonexistent.key.123"), "nonexistent.key.123");
    std::env::remove_var("CC_GATEWAY_LANG");
}
