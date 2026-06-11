#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    En,
    ZhCN,
}

impl Language {
    pub fn detect() -> Self {
        Self::detect_from(
            std::env::var("CC_GATEWAY_LANG").ok().as_deref(),
            std::env::var("LC_ALL").ok().as_deref(),
            std::env::var("LANG").ok().as_deref(),
            detect_system_locale().as_deref(),
            cfg!(windows),
        )
    }

    /// Resolve language from explicit override, env vars, and optional OS locale.
    ///
    /// On Windows, `LANG`/`LC_ALL` are often set to `en_US.UTF-8` by Git or other dev
    /// tools even when the UI locale is Chinese, so the OS locale is consulted first
    /// (after `CC_GATEWAY_LANG`).
    fn detect_from(
        cc_gateway_lang: Option<&str>,
        lc_all: Option<&str>,
        lang: Option<&str>,
        system_locale: Option<&str>,
        prefer_system_before_lang: bool,
    ) -> Self {
        if let Some(s) = cc_gateway_lang {
            return Self::from_str(s);
        }
        if prefer_system_before_lang {
            if let Some(locale) = system_locale {
                if let Some(lang) = Self::from_locale(locale) {
                    return lang;
                }
            }
        }
        if let Some(s) = lc_all {
            if let Some(lang) = Self::from_locale(s) {
                return lang;
            }
        }
        if let Some(s) = lang {
            if let Some(lang) = Self::from_locale(s) {
                return lang;
            }
        }
        if !prefer_system_before_lang {
            if let Some(locale) = system_locale {
                if let Some(lang) = Self::from_locale(locale) {
                    return lang;
                }
            }
        }
        Self::En
    }

    fn from_str(s: &str) -> Self {
        Self::from_locale(s).unwrap_or(Self::En)
    }

    fn from_locale(s: &str) -> Option<Self> {
        let s = s.to_lowercase();
        if s.starts_with("zh") {
            Some(Self::ZhCN)
        } else if s.starts_with("en") {
            Some(Self::En)
        } else {
            None
        }
    }
}

#[cfg(unix)]
fn detect_system_locale() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLocale"])
            .output()
        {
            if output.status.success() {
                let locale = String::from_utf8_lossy(&output.stdout);
                let locale = locale.trim();
                if !locale.is_empty() {
                    return Some(locale.to_string());
                }
            }
        }
        if let Ok(output) = std::process::Command::new("defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
        {
            if output.status.success() {
                let languages = String::from_utf8_lossy(&output.stdout);
                if let Some(locale) = first_apple_language(&languages) {
                    return Some(locale);
                }
            }
        }
    }
    None
}

fn first_apple_language(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        let lang = line.trim().trim_end_matches(',').trim_matches('"').trim();
        if lang.is_empty() || lang == "(" || lang == ")" {
            None
        } else {
            Some(lang.to_string())
        }
    })
}

#[cfg(windows)]
fn detect_system_locale() -> Option<String> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    extern "system" {
        fn GetUserDefaultLocaleName(lpLocaleName: *mut u16, cchLocaleName: i32) -> i32;
    }

    let mut buf = vec![0u16; 85];
    let len = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    if len > 0 {
        let os = OsString::from_wide(&buf[..(len - 1) as usize]);
        os.into_string().ok()
    } else {
        None
    }
}
