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
        if let Ok(lang) = std::env::var("LANG") {
            return Self::from_str(&lang);
        }
        if let Ok(lang) = std::env::var("LC_ALL") {
            return Self::from_str(&lang);
        }
        if let Some(lang) = detect_system_locale() {
            return Self::from_str(&lang);
        }
        Self::En
    }

    pub(crate) fn from_str(s: &str) -> Self {
        let s = s.to_lowercase();
        if s.starts_with("zh") {
            Self::ZhCN
        } else {
            Self::En
        }
    }
}

#[cfg(unix)]
fn detect_system_locale() -> Option<String> {
    None
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
