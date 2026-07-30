//! The one content digest the `captures.json` format is defined in terms of.
//!
//! A shot's `hash` is the lowercase hex SHA-256 of its PNG *bytes* — never of a
//! decoded image, so it is exactly what any capture step can compute with
//! `sha256sum`, Node's `createHash('sha256')`, or Python's `hashlib.sha256`.
//! Every other command treats a recorded hash as the source of truth and never
//! re-computes it; this module exists solely so `index` can author digests that
//! agree with those hand-rolled capture steps byte for byte.

use std::fmt::Write as _;

use sha2::{Digest as _, Sha256};

/// Lowercase hex SHA-256 of `bytes`.
pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        // Two lowercase hex nibbles per byte, matching `sha256sum` output.
        write!(hex, "{byte:02x}").expect("writing to a String cannot fail");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_published_sha256_vectors() {
        // FIPS 180-4 / RFC 6234 test vectors: the contract a consumer's
        // `sha256sum` or `createHash('sha256')` must agree with.
        assert_eq!(
            hex_sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex_sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn is_lowercase_hex_of_the_expected_length() {
        let hex = hex_sha256(b"png-bytes");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "{hex}"
        );
    }

    #[test]
    fn distinct_bytes_hash_differently() {
        assert_ne!(hex_sha256(b"a"), hex_sha256(b"b"));
    }
}
