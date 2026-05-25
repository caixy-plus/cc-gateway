#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Language {
    En,
    ZhCN,
}

impl Language {
    pub fn detect() -> Self {
        if let Ok(lang) = std::env::var("CC_GATEWAY_LANG") {
            return Self::from_str(&lang);
        }
        if let Ok(lang) = std::env::var("LC_ALL") {
            if let Some(lang) = Self::from_locale(&lang) {
                return lang;
            }
        }
        if let Ok(lang) = std::env::var("LANG") {
            if let Some(lang) = Self::from_locale(&lang) {
                return lang;
            }
        }
        if let Some(lang) = detect_system_locale() {
            if let Some(lang) = Self::from_locale(&lang) {
                return lang;
            }
        }
        Self::En
    }

    pub(crate) fn from_str(s: &str) -> Self {
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

pub(crate) fn first_apple_language(output: &str) -> Option<String> {
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
