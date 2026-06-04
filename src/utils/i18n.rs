pub mod dict;
pub mod lang;

use std::sync::atomic::{AtomicU8, Ordering};

static CURRENT_LANG: AtomicU8 = AtomicU8::new(0);

pub fn init() {
    let lang = lang::Language::detect();
    let val = match lang {
        lang::Language::En => 0,
        lang::Language::ZhCN => 1,
    };
    CURRENT_LANG.store(val, Ordering::Relaxed);
}

pub fn current_language() -> lang::Language {
    match CURRENT_LANG.load(Ordering::Relaxed) {
        1 => lang::Language::ZhCN,
        _ => lang::Language::En,
    }
}
