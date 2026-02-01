//! HEX LITERAL NORMALIZER
//! 
//! Mengubah custom hex literals (dengan huruf non-hex) menjadi valid Rust hex literals
//! 
//! Contoh:
//! - 0xMERKLE01  → hash("MERKLE01") → valid u64 hex
//! - 0xWALLET01  → hash("WALLET01") → valid u64 hex
//! - 0xALICE001  → hash("ALICE001") → valid u64 hex
//! - 0xPUBKEY_X  → hash("PUBKEY_X") → valid u64 hex
//!
//! DETERMINISTIC: Setiap identifier selalu di-hash ke nilai yang sama

use std::collections::HashMap;

/// Hash function yang deterministic untuk string → u64
/// Menggunakan simple XOR-based hash untuk konsistensi
fn hash_identifier(s: &str) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;  // FNV offset basis
    
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);  // FNV prime
    }
    
    // Ensure we have a non-zero hash
    if hash == 0 {
        hash = 0x123456789abcdef0;
    }
    
    hash
}

/// Cache untuk menyimpan mapping yang sudah dihitung
/// Untuk determinisme dan performance
struct HexNormalizerCache {
    cache: HashMap<String, u64>,
}

impl HexNormalizerCache {
    fn new() -> Self {
        HexNormalizerCache {
            cache: HashMap::new(),
        }
    }
    
    fn get_or_compute(&mut self, identifier: &str) -> u64 {
        if let Some(&val) = self.cache.get(identifier) {
            return val;
        }
        
        let hash = hash_identifier(identifier);
        self.cache.insert(identifier.to_string(), hash);
        hash
    }
}

/// Check apakah string adalah valid hex literal (0-9, a-f, A-F hanya)
fn is_valid_hex_literal(s: &str) -> bool {
    if !s.starts_with("0x") && !s.starts_with("0X") {
        return false;
    }
    
    let hex_part = &s[2..];
    if hex_part.is_empty() {
        return false;
    }
    
    hex_part.chars().all(|c| c.is_ascii_hexdigit() || c == '_')
}

/// Extract identifier dari custom hex literal
/// "0xMERKLE01" → Some("MERKLE01")
/// "0xWALLET01" → Some("WALLET01")
/// "0xALICE001" → Some("ALICE001")
fn extract_hex_identifier(s: &str) -> Option<String> {
    if !s.starts_with("0x") && !s.starts_with("0X") {
        return None;
    }
    
    let hex_part = &s[2..];
    
    // Check apakah ini valid hex atau custom
    // Valid hex: hanya mengandung 0-9, a-f, A-F, _
    // Custom: mengandung huruf lain
    let has_invalid_hex_char = hex_part.chars().any(|c| {
        !c.is_ascii_hexdigit() && c != '_' && c != 'x' && c != 'X'
    });
    
    if has_invalid_hex_char {
        Some(hex_part.to_string())
    } else {
        None
    }
}

/// Rust integer type suffixes yang valid setelah hex literal
/// Contoh: 0x11u8, 0xFFu64, 0x100usize, 0x10i32
const RUST_TYPE_SUFFIXES: &[&str] = &[
    "usize", "isize",   // pointer-sized (check longest first!)
    "u128", "i128",      // 128-bit
    "u64", "i64",        // 64-bit
    "u32", "i32",        // 32-bit
    "u16", "i16",        // 16-bit
    "u8", "i8",          // 8-bit
];

/// Strip Rust integer type suffix dari hex literal.
/// Returns (hex_without_suffix, suffix)
///
/// "0x11u8"    → ("0x11", "u8")
/// "0xFFu64"   → ("0xFF", "u64")
/// "0xDEADBEEF" → ("0xDEADBEEF", "")
/// "0xMERKLE01" → ("0xMERKLE01", "")
fn strip_rust_type_suffix(literal: &str) -> (&str, &str) {
    for suffix in RUST_TYPE_SUFFIXES {
        if literal.ends_with(suffix) {
            let prefix = &literal[..literal.len() - suffix.len()];
            // Sanity check: the part before suffix must have at least
            // one hex digit after "0x", otherwise it's not really a suffix
            // (e.g. "0xu8" is not valid — there's no hex value before u8)
            if prefix.len() > 2 {
                return (prefix, suffix);
            }
        }
    }
    (literal, "")
}

/// Normalize satu hex literal
///
/// CRITICAL BUGFIX: Must recognize Rust integer type suffixes (u8, u16, u32, etc.)
/// Before: 0x11u8 → extract_hex_identifier sees 'u' as invalid → hashes "11u8" → WRONG!
/// After:  0x11u8 → strip suffix → "0x11" is valid hex → preserve → re-attach "u8" → "0x11u8"
fn normalize_single_hex_literal(literal: &str, cache: &mut HexNormalizerCache) -> String {
    // CRITICAL BUGFIX: Strip Rust type suffix BEFORE checking validity
    // Without this, 0x11u8 is treated as custom hex because 'u' is not a hex digit
    let (hex_core, type_suffix) = strip_rust_type_suffix(literal);
    
    if let Some(identifier) = extract_hex_identifier(hex_core) {
        let hash = cache.get_or_compute(&identifier);
        // Custom hex gets hashed; re-attach type suffix if any
        // (unlikely for truly custom hex like 0xMERKLE01u8, but safe to preserve)
        format!("0x{:016x}{}", hash, type_suffix)
    } else {
        // Valid hex literal, return as-is (including original suffix)
        literal.to_string()
    }
}

/// Find dan replace semua custom hex literals dalam string
/// Menggunakan regex-like approach untuk safety
fn normalize_hex_in_string(line: &str, cache: &mut HexNormalizerCache) -> String {
    let mut result = String::new();
    let mut chars = line.chars().peekable();
    
    while let Some(ch) = chars.next() {
        if ch == '0' && (chars.peek() == Some(&'x') || chars.peek() == Some(&'X')) {
            // Potentially a hex literal
            let mut hex_candidate = String::from("0");
            hex_candidate.push(chars.next().unwrap());  // consume 'x' or 'X'
            
            // Collect hex digits (0-9, a-f, A-F, custom letters, _)
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    hex_candidate.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            
            // Now we have a complete hex candidate, normalize it
            let normalized = normalize_single_hex_literal(&hex_candidate, cache);
            result.push_str(&normalized);
        } else if ch == '\'' && chars.peek().is_some() {
            // Character literal - don't process inside it
            result.push(ch);
            if let Some(c) = chars.next() {
                result.push(c);
                if c == '\\' {
                    if let Some(escaped) = chars.next() {
                        result.push(escaped);
                    }
                }
                // Consume closing quote
                if let Some(quote) = chars.next() {
                    result.push(quote);
                }
            }
        } else if ch == '"' {
            // String literal - don't process inside it
            result.push(ch);
            let mut escaped = false;
            while let Some(c) = chars.next() {
                result.push(c);
                if c == '\\' && !escaped {
                    escaped = true;
                } else if c == '"' && !escaped {
                    break;
                } else {
                    escaped = false;
                }
            }
        } else if ch == '/' && chars.peek() == Some(&'/') {
            // Line comment - keep rest of line as-is
            result.push(ch);
            while let Some(c) = chars.next() {
                result.push(c);
            }
        } else {
            result.push(ch);
        }
    }
    
    result
}

/// Main normalization function
/// Memproses seluruh source code dan menormalisasi semua custom hex literals
pub fn normalize_hex_literals(source: &str) -> String {
    let mut cache = HexNormalizerCache::new();
    let mut result = String::new();
    
    for line in source.lines() {
        let normalized_line = normalize_hex_in_string(line, &mut cache);
        result.push_str(&normalized_line);
        result.push('\n');
    }
    
    // Remove trailing newline yang ditambahkan di loop terakhir
    if result.ends_with('\n') {
        result.pop();
    }
    
    result
}

//=============================================================================
// TESTS
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_extract_hex_identifier() {
        assert_eq!(extract_hex_identifier("0xMERKLE01"), Some("MERKLE01".to_string()));
        assert_eq!(extract_hex_identifier("0xWALLET01"), Some("WALLET01".to_string()));
        assert_eq!(extract_hex_identifier("0xALICE001"), Some("ALICE001".to_string()));
        assert_eq!(extract_hex_identifier("0xDEADBEEF"), None);  // valid hex
        assert_eq!(extract_hex_identifier("0xCAFEBABE"), None);  // valid hex
    }
    
    #[test]
    fn test_is_valid_hex_literal() {
        assert!(is_valid_hex_literal("0xDEADBEEF"));
        assert!(is_valid_hex_literal("0xCAFEBABE"));
        assert!(is_valid_hex_literal("0x1234"));
        assert!(!is_valid_hex_literal("0xMERKLE01"));
        assert!(!is_valid_hex_literal("0xWALLET01"));
    }
    
    #[test]
    fn test_normalize_deterministic() {
        let mut cache1 = HexNormalizerCache::new();
        let mut cache2 = HexNormalizerCache::new();
        
        let norm1 = normalize_single_hex_literal("0xMERKLE01", &mut cache1);
        let norm2 = normalize_single_hex_literal("0xMERKLE01", &mut cache2);
        
        // Seharusnya sama (deterministic)
        assert_eq!(norm1, norm2);
        assert!(norm1.starts_with("0x"));
        assert_ne!(norm1, "0xMERKLE01");  // Harus berbeda
    }
    
    #[test]
    fn test_normalize_preserves_valid_hex() {
        let mut cache = HexNormalizerCache::new();
        assert_eq!(normalize_single_hex_literal("0xDEADBEEF", &mut cache), "0xDEADBEEF");
        assert_eq!(normalize_single_hex_literal("0xCAFEBABE", &mut cache), "0xCAFEBABE");
    }
    
    #[test]
    fn test_normalize_in_line() {
        let line = "simple_hash(combined, 0xMERKLE01)";
        let mut cache = HexNormalizerCache::new();
        let result = normalize_hex_in_string(line, &mut cache);
        
        // Should contain normalized hex
        assert!(!result.contains("0xMERKLE01"));
        assert!(result.contains("0x"));
        assert!(result.starts_with("simple_hash(combined, 0x"));
    }
    
    #[test]
    fn test_normalize_preserves_string_literals() {
        let line = r#"println("0xMERKLE01 is {}", 0xMERKLE01)"#;
        let mut cache = HexNormalizerCache::new();
        let result = normalize_hex_in_string(line, &mut cache);
        
        // String literal should be preserved
        assert!(result.contains(r#""0xMERKLE01"#));
    }
    
    #[test]
    fn test_normalize_multiple_hex_in_line() {
        let line = "create_wallet(0xALICE001, 0xWALLET01)";
        let mut cache = HexNormalizerCache::new();
        let result = normalize_hex_in_string(line, &mut cache);
        
        // Both should be normalized
        assert!(!result.contains("0xALICE001"));
        assert!(!result.contains("0xWALLET01"));
        assert_eq!(result.matches("0x").count(), 2);
    }
    
    #[test]
    fn test_full_source_normalization() {
        let source = r#"fn merkle_hash(left u64, right u64) u64 {
    combined = left ^ (right << 32)
    simple_hash(combined, 0xMERKLE01)
}

fn create_wallet(seed u64) Wallet {
    priv_key = simple_hash(seed, 0xWALLET01)
    pub_x = simple_hash(priv_key, 0xPUBKEY_X)
}
"#;
        
        let normalized = normalize_hex_literals(source);
        
        // Custom hex should be gone
        assert!(!normalized.contains("0xMERKLE01"));
        assert!(!normalized.contains("0xWALLET01"));
        assert!(!normalized.contains("0xPUBKEY_X"));
        
        // Valid hex should remain
        assert!(normalized.contains("0x32"));
        
        // Lines should be preserved
        assert!(normalized.contains("fn merkle_hash"));
        assert!(normalized.contains("fn create_wallet"));
    }
}