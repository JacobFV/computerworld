//! `data:` URLs (RFC 2397): `data:[<media type>][;base64],<data>`, with the data
//! percent-decoded and, when the `;base64` parameter is present, base64-decoded.
//! Acid2 loads its images and a stylesheet this way; a page can put anything in one.

/// Splits a `data:` URL into its media type (lower-cased, without parameters; empty
/// means `text/plain`) and decoded bytes. `None` for anything that is not a `data:`
/// URL or whose base64 is malformed.
pub fn decode(url: &str) -> Option<(String, Vec<u8>)> {
    let rest = strip_scheme(url.trim())?;
    let comma = rest.find(',')?;
    let (meta, data) = (&rest[..comma], &rest[comma + 1..]);
    let mut params = meta.split(';');
    let mime = params.next().unwrap_or("").trim().to_ascii_lowercase();
    let base64 = params.any(|p| p.trim().eq_ignore_ascii_case("base64"));
    let bytes = percent_decode(data);
    let bytes = if base64 {
        decode_base64(&bytes)?
    } else {
        bytes
    };
    let mime = if mime.is_empty() {
        "text/plain".to_owned()
    } else {
        mime
    };
    Some((mime, bytes))
}

/// Is `url` a `data:` URL (case-insensitively)?
pub fn is_data_url(url: &str) -> bool {
    strip_scheme(url.trim()).is_some()
}

fn strip_scheme(url: &str) -> Option<&str> {
    if url.len() >= 5 && url[..5].eq_ignore_ascii_case("data:") {
        Some(&url[5..])
    } else {
        None
    }
}

/// `%XX` escapes to bytes; everything else is copied as UTF-8.
pub fn percent_decode(s: &str) -> Vec<u8> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

fn hex(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Base64 (standard and URL-safe alphabets), ignoring whitespace, tolerant of
/// missing padding as the forgiving-base64 algorithm of the WHATWG Infra standard.
pub fn decode_base64(input: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut n = 0;
    for &c in input {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\n' | b'\r' | 0x0c => continue,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        n += 6;
        if n >= 8 {
            n -= 8;
            out.push((acc >> n) as u8);
            acc &= (1 << n) - 1;
        }
    }
    if n >= 6 {
        // A dangling sextet that cannot form a byte is a malformed input.
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_with_percent_encoding() {
        let (mime, bytes) =
            decode("data:text/css,.picture%20%7B%20background%3A%20none%3B%20%7D").unwrap();
        assert_eq!(mime, "text/css");
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            ".picture { background: none; }"
        );
    }

    #[test]
    fn default_media_type() {
        let (mime, bytes) = decode("data:,hello").unwrap();
        assert_eq!(mime, "text/plain");
        assert_eq!(bytes, b"hello");
    }

    #[test]
    fn base64_with_parameters() {
        let (mime, bytes) = decode("DATA:image/png;charset=x;base64,aGVsbG8=").unwrap();
        assert_eq!(mime, "image/png");
        assert_eq!(bytes, b"hello");
        assert_eq!(decode("data:;base64,aGVsbG8").unwrap().1, b"hello");
        assert_eq!(
            decode("data:;base64,aGVs%2FbG8").unwrap().1,
            decode_base64(b"aGVs/bG8").unwrap()
        );
    }

    #[test]
    fn rejects_non_data_and_bad_base64() {
        assert!(decode("http://example.com/a.png").is_none());
        assert!(decode("data:image/png;base64,!!!").is_none());
        assert!(!is_data_url("acid2/404.html"));
        assert!(is_data_url(" data:application/x-unknown,ERROR"));
    }
}
