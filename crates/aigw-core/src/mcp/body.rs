//! Bounding what an MCP call body costs to store (Stage 152).
//!
//! Two things are worth saying about a body that does not fit: how much of it is
//! stored, and that something was dropped. Both come back from [`cut`], because
//! a silently shortened body is indistinguishable from a short one — and "the
//! response was 312 bytes" is exactly the half-fact this stage exists to stop
//! recording.

/// A body at most `limit` bytes long, and whether anything was dropped.
///
/// The cut lands on a character boundary: an MCP body is JSON, and half a
/// multi-byte character is not a body anyone can read or paste into a bug
/// report. Input that is not UTF-8 at all is summarised rather than mangled — an
/// `[binary, N bytes]` placeholder still says which call produced it.
pub fn cut(bytes: &[u8], limit: usize) -> (String, bool) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return (format!("[binary, {} bytes]", bytes.len()), true);
    };
    if text.len() <= limit {
        return (text.to_string(), false);
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (text[..end].to_string(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_under_the_limit_is_kept_whole() {
        assert_eq!(
            cut(b"{\"ok\":true}", 64),
            ("{\"ok\":true}".to_string(), false)
        );
    }

    #[test]
    fn an_ascii_body_over_the_limit_keeps_its_prefix() {
        let (body, truncated) = cut(b"abcdefghij", 4);
        assert_eq!((body.as_str(), truncated), ("abcd", true));
    }

    /// `é` is two bytes. A limit landing inside it must drop the whole
    /// character, not leave `0xC3` behind.
    #[test]
    fn a_multi_byte_character_straddling_the_limit_is_dropped() {
        let (body, truncated) = cut("aé".as_bytes(), 2);
        assert_eq!((body.as_str(), truncated), ("a", true));
        assert_eq!(cut("aé".as_bytes(), 1).0, "a");
    }

    #[test]
    fn a_limit_that_lands_exactly_on_a_boundary_keeps_the_character() {
        assert_eq!(cut("aé".as_bytes(), 3), ("aé".to_string(), false));
    }

    #[test]
    fn binary_is_summarised_not_mangled() {
        let (body, truncated) = cut(&[0xff, 0xfe, 0x00], 1024);
        assert_eq!((body.as_str(), truncated), ("[binary, 3 bytes]", true));
    }
}
