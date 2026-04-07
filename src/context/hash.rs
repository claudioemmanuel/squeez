// FNV-1a 64-bit hash. Zero-dep, ~10 lines, good distribution for short strings.
// Reference: http://www.isthe.com/chongo/tech/comp/fnv/

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

pub fn fnv1a_64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

/// First 8 hex chars of a 64-bit hash, lowercase.
pub fn short_hex(h: u64) -> String {
    format!("{:016x}", h).chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_is_offset_basis() {
        assert_eq!(fnv1a_64(b""), FNV_OFFSET);
    }

    #[test]
    fn known_vector() {
        // FNV-1a("a") = 0xaf63dc4c8601ec8c
        assert_eq!(fnv1a_64(b"a"), 0xaf63_dc4c_8601_ec8c);
    }

    #[test]
    fn different_inputs_differ() {
        assert_ne!(fnv1a_64(b"foo"), fnv1a_64(b"bar"));
    }

    #[test]
    fn short_hex_length() {
        let h = fnv1a_64(b"squeez");
        assert_eq!(short_hex(h).len(), 8);
        assert!(short_hex(h).chars().all(|c| c.is_ascii_hexdigit()));
    }
}
