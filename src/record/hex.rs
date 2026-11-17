pub fn encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn round_trip() {
        let b = [0u8, 1, 0xab, 0xff];
        assert_eq!(super::encode(&b), "0001abff");
        assert_eq!(super::decode("0001abff").unwrap(), b);
        assert!(super::decode("abc").is_none());
        assert!(super::decode("zz").is_none());
    }
}
