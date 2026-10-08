//! Office URIs as the CMS emits them, and the local names derived from them.

use percent_encoding::percent_decode_str;
use sha2::{Digest, Sha256};

const SCHEMES: [&str; 2] = ["ms-word", "ms-excel"];
/// Keeps Windows paths well below MAX_PATH; CMS file names run to ~200 characters.
pub const MAX_NAME: usize = 120;
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Edit,
    View,
}

/// Length of the "|" separator at the start of s: browsers hand it over percent-encoded (%7C).
fn separator(s: &str) -> Option<usize> {
    if s.starts_with('|') {
        Some(1)
    } else if s.len() >= 3 && s.is_char_boundary(3) && s[..3].eq_ignore_ascii_case("%7c") {
        Some(3)
    } else {
        None
    }
}

/// `<sep><letter><sep>` at the start of s, e.g. "|u|": the length of it.
fn field(s: &str, letter: char) -> Option<usize> {
    let a = separator(s)?;
    let c = s[a..].chars().next()?;
    if !c.eq_ignore_ascii_case(&letter) {
        return None;
    }
    let b = separator(&s[a + 1..])?;
    Some(a + 1 + b)
}

/// Split an Office URI (`ms-word:ofe|u|https://...`) into mode and document URL.
///
/// Only the separators are decoded; the document URL keeps its own escapes.
pub fn parse_office_uri(uri: &str) -> Result<(Mode, String), String> {
    let (scheme, rest) = uri.split_once(':').ok_or_else(|| format!("not an Office URI: {uri:?}"))?;
    if !SCHEMES.iter().any(|s| s.eq_ignore_ascii_case(scheme)) {
        return Err(format!("not an Office URI: {uri:?}"));
    }
    let letters = rest.find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(rest.len());
    let (command, url) = match field(&rest[letters..], 'u') {
        Some(len) if letters > 0 => {
            let url = &rest[letters + len..];
            // an optional default save URL follows "|s|"; unused
            let end = url.char_indices().find(|&(i, _)| field(&url[i..], 's').is_some());
            (&rest[..letters], end.map_or(url, |(i, _)| &url[..i]))
        }
        _ => ("ofe", rest),
    };
    let mode = match command.to_ascii_lowercase().as_str() {
        "ofe" => Mode::Edit,
        "ofv" => Mode::View,
        _ => return Err(format!("unsupported Office command {command:?}")),
    };
    let lower = url.to_ascii_lowercase();
    if !lower.starts_with("https://") && !lower.starts_with("http://") {
        return Err(format!("unsupported document URL {url:?}"));
    }
    Ok((mode, url.to_string()))
}

/// Local file name for a document URL, safe on Linux, macOS and Windows.
pub fn file_name(url: &str) -> String {
    let after_scheme = url.split_once("://").map_or(url, |(_, rest)| rest);
    let path = after_scheme.find('/').map_or("", |i| &after_scheme[i..]);
    let path = path.split(['?', '#']).next().unwrap_or("");
    let segment = path.rsplit('/').next().unwrap_or("");
    let decoded = percent_decode_str(segment).decode_utf8_lossy();
    let safe: String = decoded
        .chars()
        .map(|c| if "<>:\"/\\|?*".contains(c) || (c as u32) < 0x20 { '_' } else { c })
        .collect();
    let name = safe.trim_end_matches(['.', ' ']); // Windows drops trailing dots and spaces
    if name.is_empty() || name.chars().all(|c| c == '.') {
        return "document".to_string();
    }
    let (mut stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if ext.chars().count() <= 10 => (stem.to_string(), ext),
        _ => (name.to_string(), ""),
    };
    if WINDOWS_RESERVED.contains(&stem.to_uppercase().as_str()) {
        stem.insert(0, '_');
    }
    let suffix = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let room = MAX_NAME.saturating_sub(suffix.chars().count());
    let stem: String = stem.chars().take(room).collect();
    let stem = stem.trim_end_matches(['.', ' ']);
    let stem = if stem.is_empty() { "document" } else { stem };
    format!("{stem}{suffix}")
}

/// Names the session file and the working directory of a document.
pub fn session_key(url: &str) -> String {
    Sha256::digest(url.as_bytes())[..8].iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://sitefusion.cms-test.beck.de/sf6becktest/api/webdav/Asset/\
                       0123456789abcdef0123456789abcdef-184/265294/EU%20Gesetzestext%20(EU%20(1).docm";

    fn parse(uri: &str) -> Result<(Mode, String), String> {
        parse_office_uri(uri)
    }

    #[test]
    fn edit_and_view() {
        assert_eq!(parse(&format!("ms-word:ofe|u|{URL}")), Ok((Mode::Edit, URL.into())));
        assert_eq!(parse(&format!("ms-excel:ofv|u|{URL}")), Ok((Mode::View, URL.into())));
    }

    #[test]
    fn scheme_case_insensitive() {
        assert_eq!(parse(&format!("MS-WORD:OFE|u|{URL}")), Ok((Mode::Edit, URL.into())));
    }

    #[test]
    fn save_url_ignored() {
        assert_eq!(parse(&format!("ms-word:ofe|u|{URL}|s|https://x/y")), Ok((Mode::Edit, URL.into())));
        assert_eq!(parse(&format!("ms-word:ofe%7Cu%7C{URL}%7Cs%7Chttps://x/y")), Ok((Mode::Edit, URL.into())));
    }

    #[test]
    fn percent_encoded_separators_from_browser() {
        assert_eq!(parse(&format!("ms-word:ofe%7Cu%7C{URL}")), Ok((Mode::Edit, URL.into())));
        assert_eq!(parse(&format!("ms-word:ofv%7cu%7c{URL}")), Ok((Mode::View, URL.into())));
    }

    #[test]
    fn bare_url_means_edit() {
        assert_eq!(parse(&format!("ms-word:{URL}")), Ok((Mode::Edit, URL.into())));
    }

    #[test]
    fn rejects() {
        assert!(parse(&format!("https:{URL}")).is_err());
        assert!(parse(&format!("ms-powerpoint:ofe|u|{URL}")).is_err()); // the CMS has no presentations
        assert!(parse(&format!("ms-word:nft|u|{URL}")).is_err()); // new from template
        assert!(parse("ms-word:ofe|u|file:///etc/passwd").is_err());
    }

    #[test]
    fn session_key_matches_lodav_py() {
        // hashlib.sha256(b"https://h/a.docx").hexdigest()[:16]; session files carry over by it
        assert_eq!(session_key("https://h/a.docx"), "83df0dd0f8293df8");
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name(URL), "EU Gesetzestext (EU (1).docm");
        assert_eq!(file_name("https://h/a/..%2F..%2Fevil.docx"), ".._.._evil.docx");
        assert_eq!(file_name("https://h/a/Re%3A%20a%7Cb%3F%2A%22x%22.docx"), "Re_ a_b___x_.docx");
        assert_eq!(file_name("https://h/a/..%5C..%5Cevil.docx"), ".._.._evil.docx");
        assert_eq!(file_name("https://h/a/Entwurf.%20."), "Entwurf");
        assert_eq!(file_name("https://h/a/CON.docx"), "_CON.docx");
        assert_eq!(file_name("https://h/a/lpt1"), "_lpt1");
        assert_eq!(file_name("https://h/a/%2E%2E"), "document");
        assert_eq!(file_name("https://h/a/"), "document");
        assert_eq!(file_name("https://h/a/x.docx?version=2#top"), "x.docx");
    }

    #[test]
    fn long_names_truncated_keeping_extension() {
        let name = file_name(&format!("https://h/a/{}.docm", "x".repeat(300)));
        assert_eq!(name.chars().count(), 120);
        assert!(name.ends_with("xxx.docm"));
        let cms = file_name(
            "https://h/a/EU-Kapazit-tsVO%20Gesetzestext.%20Januar%202015%20-ber%20Kriterien\
             %20f-r%20Antragsteller%20hinsichtlich%20der%20Zuweisung%20von%20Eisenbahn-\
             Fahrwegkapazit-t%20und%20zur%20Aufhebung%20der%20Durchf-hrungsverordnung%20(EU%20(1).docm",
        );
        assert!(cms.chars().count() <= 120);
        assert!(cms.starts_with("EU-Kapazit-tsVO Gesetzestext"));
        assert!(cms.ends_with(".docm"));
    }
}
