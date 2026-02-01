//! cargo-rustsp v1.0.0 - Intelligent Incremental RustS+ Toolchain
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────────────────────────────────────────────────────────────────┐
//! │  INCREMENTAL COMPILATION ENGINE                                      │
//! │                                                                      │
//! │  1. Load compile.json manifest (or create fresh)                     │
//! │  2. Scan all .rss files, compute SHA-256 content hashes              │
//! │  3. Build Merkle tree of project structure (paths only)              │
//! │  4. Compare merkle root:                                             │
//! │     ├─ Same → structure unchanged, check content hashes only         │
//! │     └─ Different → structural change detected:                       │
//! │        ├─ Detect new files (compile)                                 │
//! │        ├─ Detect deleted files (purge cache)                         │
//! │        ├─ Detect renames/moves (content hash match → skip compile)   │
//! │        └─ Detect modifications (recompile)                           │
//! │  5. Compile only changed .rss → .rs into target/rustsp/             │
//! │  6. Deploy (copy) ALL cached .rs to source directories               │
//! │  7. Run cargo command (build, run, test, etc.)                       │
//! │  8. Auto-cleanup deployed .rs files from source directories          │
//! │  9. Save updated compile.json                                        │
//! └──────────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Cache Layout
//!
//! ```text
//! target/rustsp/
//! ├── compile.json          # Manifest: hashes, paths, merkle root
//! └── [mirrored .rs tree]   # Compiled Rust files mirror source layout
//!     ├── src/
//!     │   ├── main.rs
//!     │   └── lib.rs
//!     └── crates/
//!         └── core/
//!             └── src/
//!                 └── lib.rs
//! ```
//!
//! ## Zero External Dependencies
//!
//! SHA-256 and JSON handling are implemented inline for maximum portability.
//! This binary depends only on `std`.

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};

// ============================================================================
// ANSI COLORS
// ============================================================================

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const BOLD_BLUE: &str = "\x1b[1;34m";
    pub const GREEN: &str = "\x1b[32m";
    pub const CYAN: &str = "\x1b[36m";
}

// ============================================================================
// SHA-256 (Pure Rust, FIPS 180-4 compliant)
// ============================================================================

mod sha256 {
    use std::fmt::Write;

    /// Initial hash values: first 32 bits of fractional parts of
    /// the square roots of the first 8 primes (2..19)
    const H_INIT: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a,
        0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];

    /// Round constants: first 32 bits of fractional parts of
    /// the cube roots of the first 64 primes (2..311)
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
        0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
        0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
        0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
        0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
        0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
        0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
        0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];

    #[inline(always)]
    fn rotr(x: u32, n: u32) -> u32 {
        (x >> n) | (x << (32 - n))
    }

    #[inline(always)]
    fn ch(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (!x & z)
    }

    #[inline(always)]
    fn maj(x: u32, y: u32, z: u32) -> u32 {
        (x & y) ^ (x & z) ^ (y & z)
    }

    #[inline(always)]
    fn big_sigma0(x: u32) -> u32 {
        rotr(x, 2) ^ rotr(x, 13) ^ rotr(x, 22)
    }

    #[inline(always)]
    fn big_sigma1(x: u32) -> u32 {
        rotr(x, 6) ^ rotr(x, 11) ^ rotr(x, 25)
    }

    #[inline(always)]
    fn small_sigma0(x: u32) -> u32 {
        rotr(x, 7) ^ rotr(x, 18) ^ (x >> 3)
    }

    #[inline(always)]
    fn small_sigma1(x: u32) -> u32 {
        rotr(x, 17) ^ rotr(x, 19) ^ (x >> 10)
    }

    /// Compute SHA-256 digest of input bytes, returning 32 bytes
    pub fn digest(data: &[u8]) -> [u8; 32] {
        let bit_len = (data.len() as u64) * 8;

        // Pre-processing: padding
        // message + 1-bit + zeros + 64-bit length
        // Total must be multiple of 64 bytes (512 bits)
        let mut msg = data.to_vec();
        msg.push(0x80); // append bit '1' (as byte 0x80)
        while msg.len() % 64 != 56 {
            msg.push(0x00);
        }
        // Append original length as 64-bit big-endian
        msg.extend_from_slice(&bit_len.to_be_bytes());

        // Initialize hash state
        let mut h = H_INIT;

        // Process each 512-bit (64-byte) block
        for chunk in msg.chunks_exact(64) {
            // Build message schedule W[0..64]
            let mut w = [0u32; 64];
            for i in 0..16 {
                w[i] = u32::from_be_bytes([
                    chunk[i * 4],
                    chunk[i * 4 + 1],
                    chunk[i * 4 + 2],
                    chunk[i * 4 + 3],
                ]);
            }
            for i in 16..64 {
                w[i] = small_sigma1(w[i - 2])
                    .wrapping_add(w[i - 7])
                    .wrapping_add(small_sigma0(w[i - 15]))
                    .wrapping_add(w[i - 16]);
            }

            // Working variables
            let mut a = h[0];
            let mut b = h[1];
            let mut c = h[2];
            let mut d = h[3];
            let mut e = h[4];
            let mut f = h[5];
            let mut g = h[6];
            let mut hh = h[7];

            // 64 rounds of compression
            for i in 0..64 {
                let t1 = hh
                    .wrapping_add(big_sigma1(e))
                    .wrapping_add(ch(e, f, g))
                    .wrapping_add(K[i])
                    .wrapping_add(w[i]);
                let t2 = big_sigma0(a).wrapping_add(maj(a, b, c));

                hh = g;
                g = f;
                f = e;
                e = d.wrapping_add(t1);
                d = c;
                c = b;
                b = a;
                a = t1.wrapping_add(t2);
            }

            // Update hash state
            h[0] = h[0].wrapping_add(a);
            h[1] = h[1].wrapping_add(b);
            h[2] = h[2].wrapping_add(c);
            h[3] = h[3].wrapping_add(d);
            h[4] = h[4].wrapping_add(e);
            h[5] = h[5].wrapping_add(f);
            h[6] = h[6].wrapping_add(g);
            h[7] = h[7].wrapping_add(hh);
        }

        // Produce final digest
        let mut result = [0u8; 32];
        for i in 0..8 {
            let bytes = h[i].to_be_bytes();
            result[i * 4] = bytes[0];
            result[i * 4 + 1] = bytes[1];
            result[i * 4 + 2] = bytes[2];
            result[i * 4 + 3] = bytes[3];
        }
        result
    }

    /// Compute SHA-256 of bytes and return hex string
    pub fn hash_bytes(data: &[u8]) -> String {
        let d = digest(data);
        let mut hex = String::with_capacity(64);
        for byte in &d {
            let _ = write!(hex, "{:02x}", byte);
        }
        hex
    }

    /// Compute SHA-256 of a string and return hex string
    pub fn hash_str(s: &str) -> String {
        hash_bytes(s.as_bytes())
    }

    /// Compute SHA-256 of file content
    pub fn hash_file(path: &std::path::Path) -> std::io::Result<String> {
        let data = std::fs::read(path)?;
        Ok(hash_bytes(&data))
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_empty_string() {
            assert_eq!(
                hash_str(""),
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
            );
        }

        #[test]
        fn test_abc() {
            assert_eq!(
                hash_str("abc"),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
        }

        #[test]
        fn test_longer_message() {
            assert_eq!(
                hash_str("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
            );
        }
    }
}

// ============================================================================
// MINIMAL JSON PARSER & SERIALIZER
// ============================================================================
//
// Handles: strings, objects (nested), arrays of strings
// Sufficient for compile.json format. No external dependencies.

mod json {
    /// JSON value (subset sufficient for compile.json)
    #[derive(Debug, Clone)]
    pub enum JVal {
        Str(String),
        Obj(Vec<(String, JVal)>),
    }

    impl JVal {
        /// Get string value if this is JVal::Str
        pub fn as_str(&self) -> Option<&str> {
            match self {
                JVal::Str(s) => Some(s),
                _ => None,
            }
        }

        /// Get object entries if this is JVal::Obj
        pub fn as_obj(&self) -> Option<&[(String, JVal)]> {
            match self {
                JVal::Obj(entries) => Some(entries),
                _ => None,
            }
        }

        /// Look up a key in an object
        pub fn get(&self, key: &str) -> Option<&JVal> {
            self.as_obj()?.iter().find(|(k, _)| k == key).map(|(_, v)| v)
        }

        /// Look up a key and return as string
        pub fn get_str(&self, key: &str) -> Option<&str> {
            self.get(key)?.as_str()
        }

        /// Serialize to pretty JSON
        pub fn to_json(&self, indent: usize) -> String {
            match self {
                JVal::Str(s) => format!("\"{}\"", escape_json(s)),
                JVal::Obj(entries) => {
                    if entries.is_empty() {
                        return "{}".to_string();
                    }
                    let pad = " ".repeat(indent + 4);
                    let close_pad = " ".repeat(indent);
                    let mut parts = Vec::new();
                    for (k, v) in entries {
                        parts.push(format!(
                            "{}\"{}\": {}",
                            pad,
                            escape_json(k),
                            v.to_json(indent + 4)
                        ));
                    }
                    format!("{{\n{}\n{}}}", parts.join(",\n"), close_pad)
                }
            }
        }
    }

    /// Escape special characters for JSON strings
    fn escape_json(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        for c in s.chars() {
            match c {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c < '\x20' => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out
    }

    /// Recursive descent JSON parser
    pub struct Parser<'a> {
        bytes: &'a [u8],
        pos: usize,
    }

    impl<'a> Parser<'a> {
        pub fn new(input: &'a str) -> Self {
            Parser {
                bytes: input.as_bytes(),
                pos: 0,
            }
        }

        fn skip_ws(&mut self) {
            while self.pos < self.bytes.len() && self.bytes[self.pos].is_ascii_whitespace() {
                self.pos += 1;
            }
        }

        fn peek(&self) -> Option<u8> {
            self.bytes.get(self.pos).copied()
        }

        fn advance(&mut self) -> Option<u8> {
            let b = self.bytes.get(self.pos).copied()?;
            self.pos += 1;
            Some(b)
        }

        fn expect(&mut self, expected: u8) -> bool {
            self.skip_ws();
            if self.peek() == Some(expected) {
                self.pos += 1;
                true
            } else {
                false
            }
        }

        fn parse_string(&mut self) -> Option<String> {
            self.skip_ws();
            if self.advance()? != b'"' {
                return None;
            }
            let mut s = Vec::new();
            loop {
                let b = self.advance()?;
                if b == b'"' {
                    break;
                }
                if b == b'\\' {
                    let esc = self.advance()?;
                    match esc {
                        b'"' => s.push(b'"'),
                        b'\\' => s.push(b'\\'),
                        b'/' => s.push(b'/'),
                        b'n' => s.push(b'\n'),
                        b'r' => s.push(b'\r'),
                        b't' => s.push(b'\t'),
                        b'u' => {
                            // Parse \uXXXX
                            let mut hex = [0u8; 4];
                            for h in &mut hex {
                                *h = self.advance()?;
                            }
                            if let Ok(hex_str) = std::str::from_utf8(&hex) {
                                if let Ok(cp) = u32::from_str_radix(hex_str, 16) {
                                    if let Some(c) = char::from_u32(cp) {
                                        let mut buf = [0u8; 4];
                                        let encoded = c.encode_utf8(&mut buf);
                                        s.extend_from_slice(encoded.as_bytes());
                                    }
                                }
                            }
                        }
                        _ => {
                            s.push(b'\\');
                            s.push(esc);
                        }
                    }
                } else {
                    s.push(b);
                }
            }
            String::from_utf8(s).ok()
        }

        fn parse_object(&mut self) -> Option<JVal> {
            self.skip_ws();
            if self.advance()? != b'{' {
                return None;
            }
            let mut entries = Vec::new();
            self.skip_ws();
            if self.peek() == Some(b'}') {
                self.advance();
                return Some(JVal::Obj(entries));
            }
            loop {
                let key = self.parse_string()?;
                self.skip_ws();
                if self.advance()? != b':' {
                    return None;
                }
                let val = self.parse_value()?;
                entries.push((key, val));
                self.skip_ws();
                match self.peek()? {
                    b',' => {
                        self.advance();
                    }
                    b'}' => {
                        self.advance();
                        break;
                    }
                    _ => return None,
                }
            }
            Some(JVal::Obj(entries))
        }

        fn parse_value(&mut self) -> Option<JVal> {
            self.skip_ws();
            match self.peek()? {
                b'"' => self.parse_string().map(JVal::Str),
                b'{' => self.parse_object(),
                // We don't need null, bool, numbers, arrays for our format
                // but handle gracefully by skipping unknown values
                _ => {
                    // Skip unknown token (numbers, booleans, null)
                    let start = self.pos;
                    while self.pos < self.bytes.len() {
                        let b = self.bytes[self.pos];
                        if b == b',' || b == b'}' || b == b']' || b.is_ascii_whitespace() {
                            break;
                        }
                        self.pos += 1;
                    }
                    let token = std::str::from_utf8(&self.bytes[start..self.pos]).ok()?;
                    Some(JVal::Str(token.to_string()))
                }
            }
        }

        /// Parse the input as a JSON value
        pub fn parse(&mut self) -> Option<JVal> {
            self.parse_value()
        }
    }

    /// Parse a JSON string into a JVal
    pub fn parse(input: &str) -> Option<JVal> {
        Parser::new(input).parse()
    }

    /// Build a JSON object from key-value pairs
    pub fn obj(entries: Vec<(String, JVal)>) -> JVal {
        JVal::Obj(entries)
    }

    /// Build a JSON string value
    pub fn str_val(s: &str) -> JVal {
        JVal::Str(s.to_string())
    }
}

// ============================================================================
// MERKLE TREE (for project structure hashing)
// ============================================================================
//
// Leaf nodes: SHA-256 of each file's relative path
// Internal nodes: SHA-256 of concatenated sorted child hashes
// Root hash changes iff any file is added, removed, renamed, or moved.
// Content changes do NOT affect the merkle root (tracked separately).

mod merkle {
    use super::sha256;

    /// Compute the merkle root hash of a set of relative file paths.
    /// The paths are sorted for determinism.
    /// Returns a hex-encoded SHA-256 hash.
    pub fn compute_root(paths: &[String]) -> String {
        if paths.is_empty() {
            return sha256::hash_str("EMPTY_TREE");
        }

        // Sort paths for deterministic ordering
        let mut sorted: Vec<&str> = paths.iter().map(|s| s.as_str()).collect();
        sorted.sort();

        // Create leaf hashes
        let mut hashes: Vec<String> = sorted
            .iter()
            .map(|p| sha256::hash_str(&format!("LEAF:{}", p)))
            .collect();

        // Build tree bottom-up
        while hashes.len() > 1 {
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < hashes.len() {
                if i + 1 < hashes.len() {
                    // Combine pair
                    let combined = format!("NODE:{}:{}", hashes[i], hashes[i + 1]);
                    next_level.push(sha256::hash_str(&combined));
                    i += 2;
                } else {
                    // Odd node: promote as-is (wrapped for domain separation)
                    let promoted = format!("PROMOTE:{}", hashes[i]);
                    next_level.push(sha256::hash_str(&promoted));
                    i += 1;
                }
            }
            hashes = next_level;
        }

        hashes.into_iter().next().unwrap_or_default()
    }
}

// ============================================================================
// COMPILE MANIFEST (compile.json)
// ============================================================================

/// Represents a single tracked .rss file in the manifest
#[derive(Debug, Clone)]
struct FileEntry {
    /// Relative path of the .rss source file (e.g., "src/main.rss")
    source_path: String,
    /// SHA-256 hash of the .rss file content
    content_hash: String,
    /// Just the file name (e.g., "main.rss")
    file_name: String,
    /// Path to the cached .rs file in target/rustsp/ (e.g., "target/rustsp/src/main.rs")
    cached_rs: String,
    /// Path where .rs should be deployed (e.g., "src/main.rs")
    deploy_path: String,
}

/// The compile.json manifest that tracks all compilation state
#[derive(Debug, Clone)]
struct CompileManifest {
    /// Manifest format version
    version: String,
    /// Merkle root hash of project structure (paths only)
    merkle_root: String,
    /// Unix timestamp of last update
    last_updated: String,
    /// All tracked files, keyed by relative .rss path
    files: BTreeMap<String, FileEntry>,
}

impl CompileManifest {
    fn new() -> Self {
        CompileManifest {
            version: "1.0.0".to_string(),
            merkle_root: String::new(),
            last_updated: Self::timestamp(),
            files: BTreeMap::new(),
        }
    }

    fn timestamp() -> String {
        match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_secs().to_string(),
            Err(_) => "0".to_string(),
        }
    }

    /// Load manifest from compile.json
    fn load(path: &Path) -> Option<Self> {
        let content = fs::read_to_string(path).ok()?;
        let root = json::parse(&content)?;

        let version = root.get_str("version")?.to_string();
        // Version check - reject incompatible formats
        if !version.starts_with("2.") {
            return None;
        }

        let merkle_root = root.get_str("merkle_root").unwrap_or("").to_string();
        let last_updated = root.get_str("last_updated").unwrap_or("0").to_string();

        let mut files = BTreeMap::new();
        if let Some(files_obj) = root.get("files") {
            if let Some(entries) = files_obj.as_obj() {
                for (key, val) in entries {
                    let content_hash = val.get_str("content_hash").unwrap_or("").to_string();
                    let file_name = val.get_str("file_name").unwrap_or("").to_string();
                    let cached_rs = val.get_str("cached_rs").unwrap_or("").to_string();
                    let deploy_path = val.get_str("deploy_path").unwrap_or("").to_string();

                    files.insert(
                        key.clone(),
                        FileEntry {
                            source_path: key.clone(),
                            content_hash,
                            file_name,
                            cached_rs,
                            deploy_path,
                        },
                    );
                }
            }
        }

        Some(CompileManifest {
            version,
            merkle_root,
            last_updated,
            files,
        })
    }

    /// Save manifest to compile.json
    fn save(&self, path: &Path) -> io::Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Build JSON manually using our json module
        let mut file_entries = Vec::new();
        for (key, entry) in &self.files {
            let entry_obj = json::obj(vec![
                ("content_hash".to_string(), json::str_val(&entry.content_hash)),
                ("file_name".to_string(), json::str_val(&entry.file_name)),
                ("cached_rs".to_string(), json::str_val(&entry.cached_rs)),
                ("deploy_path".to_string(), json::str_val(&entry.deploy_path)),
            ]);
            file_entries.push((key.clone(), entry_obj));
        }

        let root = json::obj(vec![
            ("version".to_string(), json::str_val(&self.version)),
            ("merkle_root".to_string(), json::str_val(&self.merkle_root)),
            ("last_updated".to_string(), json::str_val(&self.last_updated)),
            ("file_count".to_string(), json::str_val(&self.files.len().to_string())),
            ("files".to_string(), json::obj(file_entries)),
        ]);

        let json_str = root.to_json(0);
        fs::write(path, json_str.as_bytes())
    }
}

// ============================================================================
// CHANGE DETECTION ENGINE
// ============================================================================

/// Describes what kind of change occurred to a file
#[derive(Debug)]
enum ChangeKind {
    /// File is new (not in manifest)
    New,
    /// File content changed (hash mismatch)
    Modified,
    /// File was renamed (same content, different name, same directory)
    Renamed { old_path: String },
    /// File was moved (same content + name, different directory)
    Moved { old_path: String },
    /// File was moved AND renamed
    MovedRenamed { old_path: String },
    /// File exists in manifest but is gone from disk
    Deleted,
    /// No changes detected
    Unchanged,
}

impl ChangeKind {
    fn needs_compile(&self) -> bool {
        matches!(self, ChangeKind::New | ChangeKind::Modified)
    }

    fn display(&self) -> &str {
        match self {
            ChangeKind::New => "NEW",
            ChangeKind::Modified => "MODIFIED",
            ChangeKind::Renamed { .. } => "RENAMED",
            ChangeKind::Moved { .. } => "MOVED",
            ChangeKind::MovedRenamed { .. } => "MOVED+RENAMED",
            ChangeKind::Deleted => "DELETED",
            ChangeKind::Unchanged => "UNCHANGED",
        }
    }
}

/// Information about a currently scanned file
#[derive(Debug, Clone)]
struct ScannedFile {
    /// Relative path from project root
    relative_path: String,
    /// SHA-256 of file content
    content_hash: String,
    /// Just the file name
    file_name: String,
}

/// Result of change detection between manifest and current project state
struct ChangeSet {
    /// Changes per file path (current paths for existing files, old paths for deleted)
    changes: Vec<(String, ChangeKind)>,
    /// Whether the project structure changed (merkle root mismatch)
    structure_changed: bool,
    /// New merkle root
    new_merkle_root: String,
}

/// Detect all changes between the saved manifest and the current project state.
///
/// This is the core intelligence of the incremental compiler. It:
/// 1. Compares merkle roots for fast structure check
/// 2. Cross-references content hashes to detect renames/moves
/// 3. Identifies new, modified, deleted, and unchanged files
fn detect_changes(
    manifest: &CompileManifest,
    current_files: &BTreeMap<String, ScannedFile>,
) -> ChangeSet {
    // Compute new merkle root
    let current_paths: Vec<String> = current_files.keys().cloned().collect();
    let new_merkle = merkle::compute_root(&current_paths);
    let structure_changed = manifest.merkle_root != new_merkle;

    let mut changes: Vec<(String, ChangeKind)> = Vec::new();

    // Build reverse lookup: content_hash → manifest entry path
    // Used for rename/move detection
    let mut hash_to_old_path: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (path, entry) in &manifest.files {
        hash_to_old_path
            .entry(entry.content_hash.clone())
            .or_default()
            .push(path.clone());
    }

    // Track which old paths have been "claimed" by rename/move detection
    let mut claimed_old_paths: std::collections::HashSet<String> = std::collections::HashSet::new();

    // Phase 1: Analyze each current file
    for (current_path, scanned) in current_files {
        if let Some(old_entry) = manifest.files.get(current_path) {
            // Path exists in both old and new
            if old_entry.content_hash == scanned.content_hash {
                changes.push((current_path.clone(), ChangeKind::Unchanged));
            } else {
                changes.push((current_path.clone(), ChangeKind::Modified));
            }
            claimed_old_paths.insert(current_path.clone());
        } else {
            // Path is new - but check if it's a rename/move by content hash
            let mut found_match = false;
            if let Some(old_paths) = hash_to_old_path.get(&scanned.content_hash) {
                for old_path in old_paths {
                    // Only match if the old path is actually missing from current files
                    // AND hasn't been claimed already
                    if !current_files.contains_key(old_path) && !claimed_old_paths.contains(old_path) {
                        let old_entry = &manifest.files[old_path];
                        let same_name = old_entry.file_name == scanned.file_name;
                        let same_dir = {
                            let old_dir = Path::new(old_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            let new_dir = Path::new(current_path).parent().map(|p| p.to_string_lossy().to_string()).unwrap_or_default();
                            old_dir == new_dir
                        };

                        let change = if same_name && !same_dir {
                            ChangeKind::Moved { old_path: old_path.clone() }
                        } else if !same_name && same_dir {
                            ChangeKind::Renamed { old_path: old_path.clone() }
                        } else {
                            ChangeKind::MovedRenamed { old_path: old_path.clone() }
                        };

                        changes.push((current_path.clone(), change));
                        claimed_old_paths.insert(old_path.clone());
                        found_match = true;
                        break;
                    }
                }
            }

            if !found_match {
                changes.push((current_path.clone(), ChangeKind::New));
            }
        }
    }

    // Phase 2: Find deleted files (in manifest but not in current, and not claimed by rename/move)
    for old_path in manifest.files.keys() {
        if !current_files.contains_key(old_path) && !claimed_old_paths.contains(old_path) {
            changes.push((old_path.clone(), ChangeKind::Deleted));
        }
    }

    ChangeSet {
        changes,
        structure_changed,
        new_merkle_root: new_merkle,
    }
}

// ============================================================================
// DEPLOY TRACKER (auto-cleanup of deployed .rs files)
// ============================================================================

/// Tracks deployed .rs files and cleans them up when dropped.
/// This ensures the source tree stays clean even if cargo fails.
struct DeployTracker {
    deployed: Vec<PathBuf>,
    quiet: bool,
    keep: bool,
}

impl DeployTracker {
    fn new(quiet: bool, keep: bool) -> Self {
        DeployTracker {
            deployed: Vec::new(),
            quiet,
            keep,
        }
    }

    /// Deploy (copy) a cached .rs file to its source directory location
    fn deploy(&mut self, project_root: &Path, cached_rs: &Path, deploy_path: &Path) -> io::Result<()> {
        let full_deploy = project_root.join(deploy_path);
        if let Some(parent) = full_deploy.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(cached_rs, &full_deploy)?;
        self.deployed.push(full_deploy);
        Ok(())
    }

    fn cleanup(&mut self) {
        if self.keep {
            if !self.quiet && !self.deployed.is_empty() {
                eprintln!(
                    "{}      Keeping{} {} deployed .rs file(s) (--rustsp-keep)",
                    ansi::BOLD_YELLOW,
                    ansi::RESET,
                    self.deployed.len()
                );
            }
            return;
        }

        let mut cleaned = 0;
        for path in &self.deployed {
            if path.exists() {
                if fs::remove_file(path).is_ok() {
                    cleaned += 1;
                }
            }
        }

        if !self.quiet && cleaned > 0 {
            eprintln!(
                "{}     Cleanup{} removed {} deployed .rs file(s) from source tree",
                ansi::BOLD_GREEN,
                ansi::RESET,
                cleaned
            );
        }

        self.deployed.clear();
    }

    fn file_count(&self) -> usize {
        self.deployed.len()
    }
}

impl Drop for DeployTracker {
    fn drop(&mut self) {
        self.cleanup();
    }
}

// ============================================================================
// INCREMENTAL COMPILER ENGINE
// ============================================================================

struct IncrementalCompiler {
    project_root: PathBuf,
    cache_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: CompileManifest,
    rustsp_binary: String,
    quiet: bool,
    force: bool,
}

impl IncrementalCompiler {
    fn new(project_root: PathBuf, quiet: bool, force: bool) -> Self {
        let cache_dir = project_root.join("target").join("rustsp");
        let manifest_path = cache_dir.join("compile.json");

        // Load existing manifest or create fresh
        let manifest = if force {
            CompileManifest::new()
        } else {
            CompileManifest::load(&manifest_path).unwrap_or_else(CompileManifest::new)
        };

        let rustsp_binary = find_rustsp_binary();

        IncrementalCompiler {
            project_root,
            cache_dir,
            manifest_path,
            manifest,
            rustsp_binary,
            quiet,
            force,
        }
    }

    /// Scan the project for all .rss files and compute their hashes
    fn scan_project(&self) -> BTreeMap<String, ScannedFile> {
        let rss_files = find_rss_files(&self.project_root);
        let mut scanned = BTreeMap::new();

        for rss_path in rss_files {
            // Compute relative path (using forward slashes for consistency)
            let relative = match rss_path.strip_prefix(&self.project_root) {
                Ok(rel) => normalize_path(&rel.to_string_lossy()),
                Err(_) => continue,
            };

            // Compute content hash
            let content_hash = match sha256::hash_file(&rss_path) {
                Ok(h) => h,
                Err(_) => continue, // Skip unreadable files
            };

            let file_name = rss_path
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();

            scanned.insert(
                relative.clone(),
                ScannedFile {
                    relative_path: relative,
                    content_hash,
                    file_name,
                },
            );
        }

        scanned
    }

    /// Compute the cached .rs path for a given .rss source path
    fn cached_rs_path(&self, relative_rss: &str) -> PathBuf {
        // Mirror source structure in target/rustsp/
        // e.g., "src/main.rss" → "target/rustsp/src/main.rs"
        let rs_relative = relative_rss.replace(".rss", ".rs");
        self.cache_dir.join(&rs_relative)
    }

    /// Compute the deploy path (where .rs goes alongside .rss in source)
    fn deploy_path(relative_rss: &str) -> String {
        relative_rss.replace(".rss", ".rs")
    }

    /// Compile a single .rss file using the RustS+ compiler
    fn compile_file(&self, rss_path: &Path, output_rs: &Path) -> Result<(), String> {
        // Ensure output directory exists
        if let Some(parent) = output_rs.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create cache directory: {}", e))?;
        }

        let result = Command::new(&self.rustsp_binary)
            .arg(rss_path)
            .arg("--emit-rs")
            .arg("-o")
            .arg(output_rs)
            .output()
            .map_err(|e| format!("Failed to run {}: {}", self.rustsp_binary, e))?;

        if !result.status.success() {
            let stderr = String::from_utf8_lossy(&result.stderr);
            let stdout = String::from_utf8_lossy(&result.stdout);
            // ALWAYS print compilation errors (never suppress)
            if !stderr.is_empty() {
                eprintln!("{}", stderr);
            }
            if !stdout.is_empty() {
                eprintln!("{}", stdout);
            }
            return Err(format!("Compilation failed: {}", rss_path.display()));
        }

        if !output_rs.exists() {
            return Err(format!(
                "Compiler did not create output: {}",
                output_rs.display()
            ));
        }

        Ok(())
    }

    /// Main incremental compilation process.
    /// Returns a DeployTracker with all .rs files deployed to source directories.
    fn process(&mut self, keep: bool) -> Result<DeployTracker, String> {
        let mut tracker = DeployTracker::new(self.quiet, keep);

        // Step 1: Scan all current .rss files
        let current_files = self.scan_project();

        if current_files.is_empty() {
            if !self.quiet {
                eprintln!(
                    "  {}(no .rss files found){}",
                    ansi::DIM,
                    ansi::RESET
                );
            }
            // If we had files before but now have none, clear manifest
            if !self.manifest.files.is_empty() {
                self.manifest = CompileManifest::new();
                let _ = self.manifest.save(&self.manifest_path);
            }
            return Ok(tracker);
        }

        // Step 2: Detect changes
        let is_fresh = self.manifest.files.is_empty() || self.force;
        let change_set = if is_fresh {
            // Fresh compile: everything is new
            let paths: Vec<String> = current_files.keys().cloned().collect();
            let new_merkle = merkle::compute_root(&paths);
            ChangeSet {
                changes: current_files
                    .keys()
                    .map(|p| (p.clone(), ChangeKind::New))
                    .collect(),
                structure_changed: true,
                new_merkle_root: new_merkle,
            }
        } else {
            detect_changes(&self.manifest, &current_files)
        };

        // Step 3: Summarize changes
        let mut to_compile: Vec<String> = Vec::new();
        let mut to_update_cache: Vec<(String, String)> = Vec::new(); // (new_path, old_path)
        let mut to_delete: Vec<String> = Vec::new();
        let mut unchanged_count: usize = 0;

        for (path, change) in &change_set.changes {
            match change {
                ChangeKind::New => {
                    to_compile.push(path.clone());
                    if !self.quiet {
                        eprintln!(
                            "      {}[NEW]{} {}",
                            ansi::BOLD_GREEN,
                            ansi::RESET,
                            path
                        );
                    }
                }
                ChangeKind::Modified => {
                    to_compile.push(path.clone());
                    if !self.quiet {
                        eprintln!(
                            "      {}[MOD]{} {}",
                            ansi::BOLD_YELLOW,
                            ansi::RESET,
                            path
                        );
                    }
                }
                ChangeKind::Renamed { old_path }
                | ChangeKind::Moved { old_path }
                | ChangeKind::MovedRenamed { old_path } => {
                    to_update_cache.push((path.clone(), old_path.clone()));
                    if !self.quiet {
                        eprintln!(
                            "   {}[{}]{} {} ← {}",
                            ansi::BOLD_CYAN,
                            change.display(),
                            ansi::RESET,
                            path,
                            old_path
                        );
                    }
                }
                ChangeKind::Deleted => {
                    to_delete.push(path.clone());
                    if !self.quiet {
                        eprintln!(
                            "      {}[DEL]{} {}",
                            ansi::BOLD_RED,
                            ansi::RESET,
                            path
                        );
                    }
                }
                ChangeKind::Unchanged => {
                    unchanged_count += 1;
                }
            }
        }

        // Step 4: Handle deletions - remove stale cache entries and files
        for del_path in &to_delete {
            if let Some(entry) = self.manifest.files.get(del_path) {
                let cached = self.project_root.join(&entry.cached_rs);
                if cached.exists() {
                    let _ = fs::remove_file(&cached);
                }
            }
            self.manifest.files.remove(del_path);
        }

        // Step 5: Handle renames/moves - update cache paths without recompiling
        for (new_path, old_path) in &to_update_cache {
            if let Some(mut entry) = self.manifest.files.remove(old_path) {
                let old_cached = self.project_root.join(&entry.cached_rs);
                let new_cached = self.cached_rs_path(new_path);

                // Move the cached .rs file to new location
                if old_cached.exists() {
                    if let Some(parent) = new_cached.parent() {
                        let _ = fs::create_dir_all(parent);
                    }
                    let _ = fs::rename(&old_cached, &new_cached);
                }

                // Update entry with new paths
                let scanned = &current_files[new_path];
                entry.source_path = new_path.clone();
                entry.file_name = scanned.file_name.clone();
                entry.cached_rs = normalize_path(
                    &new_cached
                        .strip_prefix(&self.project_root)
                        .unwrap_or(&new_cached)
                        .to_string_lossy(),
                );
                entry.deploy_path = Self::deploy_path(new_path);

                self.manifest.files.insert(new_path.clone(), entry);
            }
        }

        // Step 6: Compile changed/new files
        let mut compiled = 0;
        let mut all_errors: Vec<(String, String)> = Vec::new();

        for compile_path in &to_compile {
            let scanned = &current_files[compile_path];
            let full_rss = self.project_root.join(compile_path);
            let cached_rs = self.cached_rs_path(compile_path);

            if !self.quiet {
                eprintln!(
                    "   {}Compiling{} {}",
                    ansi::DIM,
                    ansi::RESET,
                    compile_path
                );
            }

            match self.compile_file(&full_rss, &cached_rs) {
                Ok(()) => {
                    compiled += 1;

                    // Update manifest entry
                    let cached_relative = normalize_path(
                        &cached_rs
                            .strip_prefix(&self.project_root)
                            .unwrap_or(&cached_rs)
                            .to_string_lossy(),
                    );

                    self.manifest.files.insert(
                        compile_path.clone(),
                        FileEntry {
                            source_path: compile_path.clone(),
                            content_hash: scanned.content_hash.clone(),
                            file_name: scanned.file_name.clone(),
                            cached_rs: cached_relative,
                            deploy_path: Self::deploy_path(compile_path),
                        },
                    );
                }
                Err(e) => {
                    all_errors.push((compile_path.clone(), e));
                }
            }
        }

        // Step 7: Verify all cached .rs files exist (handle manual deletion)
        let mut recompile_missing: Vec<String> = Vec::new();
        for (path, entry) in &self.manifest.files {
            if to_compile.contains(path) || to_delete.contains(path) {
                continue; // Already handled
            }
            let cached = self.project_root.join(&entry.cached_rs);
            if !cached.exists() {
                recompile_missing.push(path.clone());
            }
        }

        for missing_path in &recompile_missing {
            let scanned = match current_files.get(missing_path) {
                Some(s) => s,
                None => continue,
            };
            let full_rss = self.project_root.join(missing_path);
            let cached_rs = self.cached_rs_path(missing_path);

            if !self.quiet {
                eprintln!(
                    "   {}Compiling{} {} (cache miss)",
                    ansi::DIM,
                    ansi::RESET,
                    missing_path
                );
            }

            match self.compile_file(&full_rss, &cached_rs) {
                Ok(()) => {
                    compiled += 1;
                    let cached_relative = normalize_path(
                        &cached_rs
                            .strip_prefix(&self.project_root)
                            .unwrap_or(&cached_rs)
                            .to_string_lossy(),
                    );
                    if let Some(entry) = self.manifest.files.get_mut(missing_path) {
                        entry.content_hash = scanned.content_hash.clone();
                        entry.cached_rs = cached_relative;
                    }
                }
                Err(e) => {
                    all_errors.push((missing_path.clone(), e));
                }
            }
        }

        // Step 8: Check for errors
        if !all_errors.is_empty() {
            eprintln!(
                "\n{}╔═══════════════════════════════════════════════════════════════╗{}",
                ansi::BOLD_RED,
                ansi::RESET
            );
            eprintln!(
                "{}║   RUSTS+ COMPILATION ERRORS ({} file(s) failed)               ║{}",
                ansi::BOLD_RED,
                all_errors.len(),
                ansi::RESET
            );
            eprintln!(
                "{}╚═══════════════════════════════════════════════════════════════╝{}\n",
                ansi::BOLD_RED,
                ansi::RESET
            );

            eprintln!("{}Failed files:{}", ansi::BOLD_RED, ansi::RESET);
            for (path, _) in &all_errors {
                eprintln!("  • {}", path);
            }
            eprintln!();

            // Cleanup any deployed files before failing
            tracker.cleanup();

            return Err(format!(
                "{} RustS+ file(s) failed to compile. Fix all errors above and try again.",
                all_errors.len()
            ));
        }

        // Step 9: Update manifest metadata
        self.manifest.merkle_root = change_set.new_merkle_root;
        self.manifest.last_updated = CompileManifest::timestamp();

        // Save manifest
        if let Err(e) = self.manifest.save(&self.manifest_path) {
            eprintln!(
                "{}warning{}: failed to save compile.json: {}",
                ansi::BOLD_YELLOW,
                ansi::RESET,
                e
            );
        }

        // Step 10: Deploy ALL cached .rs files to source directories
        let total = self.manifest.files.len();
        let mut deployed = 0;

        for entry in self.manifest.files.values() {
            let cached_rs = self.project_root.join(&entry.cached_rs);
            let deploy_path = Path::new(&entry.deploy_path);

            if !cached_rs.exists() {
                eprintln!(
                    "{}warning{}: cached file missing: {}",
                    ansi::BOLD_YELLOW,
                    ansi::RESET,
                    entry.cached_rs
                );
                continue;
            }

            match tracker.deploy(&self.project_root, &cached_rs, deploy_path) {
                Ok(()) => deployed += 1,
                Err(e) => {
                    eprintln!(
                        "{}warning{}: failed to deploy {}: {}",
                        ansi::BOLD_YELLOW,
                        ansi::RESET,
                        entry.deploy_path,
                        e
                    );
                }
            }
        }

        // Print summary
        if !self.quiet {
            let renames = to_update_cache.len();
            let deletes = to_delete.len();

            if compiled == 0 && renames == 0 && deletes == 0 {
                eprintln!(
                    "{}  Preprocessed{} {} file(s) unchanged, 0 compiled (all cached ✓)",
                    ansi::BOLD_GREEN,
                    ansi::RESET,
                    unchanged_count
                );
            } else {
                let mut parts = Vec::new();
                if compiled > 0 {
                    parts.push(format!("{} compiled", compiled));
                }
                if unchanged_count > 0 {
                    parts.push(format!("{} cached", unchanged_count));
                }
                if renames > 0 {
                    parts.push(format!("{} renamed/moved", renames));
                }
                if deletes > 0 {
                    parts.push(format!("{} removed", deletes));
                }
                let missing_recompiled = recompile_missing.len();
                if missing_recompiled > 0 {
                    parts.push(format!("{} cache-miss", missing_recompiled));
                }

                eprintln!(
                    "{}  Preprocessed{} {}",
                    ansi::BOLD_GREEN,
                    ansi::RESET,
                    parts.join(", ")
                );
            }

            if change_set.structure_changed && !is_fresh {
                eprintln!(
                    "{}  Structure{} project layout changed (merkle root updated)",
                    ansi::BOLD_CYAN,
                    ansi::RESET
                );
            }

            eprintln!(
                "{}    Deployed{} {}/{} .rs file(s) to source tree",
                ansi::BOLD_GREEN,
                ansi::RESET,
                deployed,
                total
            );
        }

        Ok(tracker)
    }
}

// ============================================================================
// CORE UTILITY FUNCTIONS
// ============================================================================

/// Normalize a path string to use forward slashes (cross-platform consistency)
fn normalize_path(p: &str) -> String {
    p.replace('\\', "/")
}

/// Find project root by looking for Cargo.toml
fn find_project_root() -> Option<PathBuf> {
    let mut current = env::current_dir().ok()?;
    loop {
        if current.join("Cargo.toml").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Find rustsp compiler binary
fn find_rustsp_binary() -> String {
    // Check common names in PATH
    for cmd in &["rustsp", "rusts_plus", "rustsp.exe", "rusts_plus.exe"] {
        if Command::new(cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
        {
            return cmd.to_string();
        }
    }

    // Check next to cargo-rustsp executable
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for name in &["rustsp", "rusts_plus", "rustsp.exe", "rusts_plus.exe"] {
                let path = dir.join(name);
                if path.exists() {
                    return path.to_string_lossy().to_string();
                }
            }
        }
    }

    "rustsp".to_string()
}

/// Recursively find all .rss files in a directory (skips target/)
fn find_rss_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    find_rss_recursive(dir, &mut files);
    files.sort(); // Deterministic order
    files
}

fn find_rss_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // Skip target, hidden dirs, and common non-source dirs
            if name != "target" && !name.starts_with('.') && name != "node_modules" {
                find_rss_recursive(&path, files);
            }
        } else if path.extension().map(|e| e == "rss").unwrap_or(false) {
            files.push(path);
        }
    }
}

/// Manually clean all deployed .rs files (those with corresponding .rss)
fn clean_deployed_files(project_root: &Path) -> usize {
    let mut cleaned = 0;

    fn clean_recursive(dir: &Path, cleaned: &mut usize) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if name != "target" && !name.starts_with('.') {
                    clean_recursive(&path, cleaned);
                }
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                // Only remove if there's a corresponding .rss file
                let rss_path = path.with_extension("rss");
                if rss_path.exists() {
                    if fs::remove_file(&path).is_ok() {
                        *cleaned += 1;
                    }
                }
            }
        }
    }

    clean_recursive(project_root, &mut cleaned);
    cleaned
}

/// Purge all empty directories inside a path (bottom-up cleanup)
fn cleanup_empty_dirs(dir: &Path) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            cleanup_empty_dirs(&path);
            // Try to remove - will only succeed if empty
            let _ = fs::remove_dir(&path);
        }
    }
}

// ============================================================================
// MAIN ENTRY POINT
// ============================================================================

fn print_usage() {
    eprintln!(
        "{}cargo-rustsp{} v1.0.0 - Intelligent Incremental RustS+ Toolchain",
        ansi::BOLD_CYAN,
        ansi::RESET
    );
    eprintln!();
    eprintln!(
        "{}USAGE:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    cargo rustsp [CARGO_COMMAND] [ARGS...]");
    eprintln!();
    eprintln!(
        "{}HOW IT WORKS:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    1. Scans all .rss files and computes SHA-256 content hashes");
    eprintln!("    2. Compares with compile.json manifest (Merkle tree + per-file hashes)");
    eprintln!("    3. Compiles ONLY changed/new .rss files → cached in target/rustsp/");
    eprintln!("    4. Deploys cached .rs files to source directories");
    eprintln!("    5. Runs cargo with your exact arguments");
    eprintln!(
        "    6. {}AUTO-CLEANS{} deployed .rs files after cargo finishes",
        ansi::BOLD_GREEN,
        ansi::RESET
    );
    eprintln!();
    eprintln!("    Your source tree stays clean - only .rss files remain!");
    eprintln!();
    eprintln!(
        "{}CHANGE DETECTION:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    • Content changes  → SHA-256 hash comparison per file");
    eprintln!("    • Structural changes → Merkle tree root comparison");
    eprintln!("    • File renames      → Content hash matching (no recompile!)");
    eprintln!("    • File moves        → Content hash matching (no recompile!)");
    eprintln!("    • New files         → Detected & compiled automatically");
    eprintln!("    • Deleted files     → Cache entry purged automatically");
    eprintln!();
    eprintln!(
        "{}EXAMPLES:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    cargo rustsp build");
    eprintln!("    cargo rustsp build --workspace --release");
    eprintln!("    cargo rustsp test --test my_test -- --ignored --nocapture");
    eprintln!("    cargo rustsp run -p my-crate --bin my-bin -- arg1 arg2");
    eprintln!("    cargo rustsp run -p dsdn-coordinator");
    eprintln!("    cargo rustsp doc --no-deps");
    eprintln!("    cargo rustsp clippy -- -W warnings");
    eprintln!("    cargo rustsp check");
    eprintln!("    cargo rustsp bench");
    eprintln!();
    eprintln!(
        "{}RUSTSP-SPECIFIC OPTIONS:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    --rustsp-force    Force recompile all .rss files (ignore cache)");
    eprintln!("    --rustsp-quiet    Suppress rustsp preprocessing output");
    eprintln!("    --rustsp-keep     Keep deployed .rs files (don't auto-clean)");
    eprintln!("    --rustsp-clean    Manually clean any leftover .rs files and exit");
    eprintln!(
        "    --rustsp-reset    {}Reset cache entirely{}: delete target/rustsp/ and exit",
        ansi::BOLD_RED,
        ansi::RESET
    );
    eprintln!("    --rustsp-status   Show cache status and manifest info");
    eprintln!();
    eprintln!(
        "{}CACHE LAYOUT:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    target/rustsp/");
    eprintln!("    ├── compile.json      Manifest (SHA-256 hashes, Merkle root, mappings)");
    eprintln!("    └── [source tree]     Cached compiled .rs files");
    eprintln!();
    eprintln!(
        "{}NOTE:{}",
        ansi::BOLD,
        ansi::RESET
    );
    eprintln!("    Any cargo command works! cargo-rustsp is a transparent wrapper.");
    eprintln!("    First compile is full (builds manifest). Subsequent compiles are incremental.");
}

/// Print cache status information
fn print_status(project_root: &Path) {
    let cache_dir = project_root.join("target").join("rustsp");
    let manifest_path = cache_dir.join("compile.json");

    eprintln!(
        "{}cargo-rustsp{} v1.0.0 - Cache Status",
        ansi::BOLD_CYAN,
        ansi::RESET
    );
    eprintln!();

    if !manifest_path.exists() {
        eprintln!(
            "  {}Status:{} No cache found (first compile will be full)",
            ansi::BOLD,
            ansi::RESET
        );
        return;
    }

    match CompileManifest::load(&manifest_path) {
        Some(manifest) => {
            eprintln!(
                "  {}Manifest:{} {}",
                ansi::BOLD,
                ansi::RESET,
                manifest_path.display()
            );
            eprintln!(
                "  {}Version:{} {}",
                ansi::BOLD,
                ansi::RESET,
                manifest.version
            );
            eprintln!(
                "  {}Merkle Root:{} {}",
                ansi::BOLD,
                ansi::RESET,
                &manifest.merkle_root[..16.min(manifest.merkle_root.len())]
            );
            eprintln!(
                "  {}Last Updated:{} {}",
                ansi::BOLD,
                ansi::RESET,
                manifest.last_updated
            );
            eprintln!(
                "  {}Cached Files:{} {}",
                ansi::BOLD,
                ansi::RESET,
                manifest.files.len()
            );

            eprintln!();
            if !manifest.files.is_empty() {
                eprintln!("  {}Files:{}", ansi::BOLD, ansi::RESET);
                for (path, entry) in &manifest.files {
                    let cached = project_root.join(&entry.cached_rs);
                    let status = if cached.exists() {
                        format!("{}✓{}", ansi::GREEN, ansi::RESET)
                    } else {
                        format!("{}✗ (missing){}", ansi::BOLD_RED, ansi::RESET)
                    };
                    eprintln!(
                        "    {} {} [{}..] {}",
                        status,
                        path,
                        &entry.content_hash[..12],
                        entry.file_name
                    );
                }
            }

            // Also scan current files and show diff
            let rss_files = find_rss_files(project_root);
            let current_count = rss_files.len();
            let cached_count = manifest.files.len();

            eprintln!();
            eprintln!(
                "  {}Current .rss files:{} {}",
                ansi::BOLD,
                ansi::RESET,
                current_count
            );
            if current_count != cached_count {
                eprintln!(
                    "  {}⚠ Mismatch:{} {} cached vs {} on disk (run build to sync)",
                    ansi::BOLD_YELLOW,
                    ansi::RESET,
                    cached_count,
                    current_count
                );
            } else {
                eprintln!(
                    "  {}✓ In sync:{} all files accounted for",
                    ansi::BOLD_GREEN,
                    ansi::RESET
                );
            }

            // Cache size
            if let Ok(size) = dir_size(&cache_dir) {
                eprintln!(
                    "  {}Cache Size:{} {}",
                    ansi::BOLD,
                    ansi::RESET,
                    format_size(size)
                );
            }
        }
        None => {
            eprintln!(
                "  {}Status:{} compile.json exists but is invalid or incompatible version",
                ansi::BOLD_YELLOW,
                ansi::RESET
            );
            eprintln!("  Run with --rustsp-reset to start fresh.");
        }
    }
}

/// Calculate total size of a directory
fn dir_size(path: &Path) -> io::Result<u64> {
    let mut total = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                total += dir_size(&path)?;
            } else {
                total += entry.metadata()?.len();
            }
        }
    }
    Ok(total)
}

/// Format byte size as human-readable
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Skip "cargo" if invoked as "cargo rustsp"
    let start_idx = if args.len() > 1 && args[1] == "rustsp" {
        2
    } else {
        1
    };

    // Extract rustsp-specific flags
    let mut force_rebuild = false;
    let mut rustsp_quiet = false;
    let mut keep_generated = false;
    let mut clean_only = false;
    let mut reset_cache = false;
    let mut show_status = false;
    let mut cargo_args: Vec<String> = Vec::new();

    for arg in args.iter().skip(start_idx) {
        match arg.as_str() {
            "--rustsp-force" => force_rebuild = true,
            "--rustsp-quiet" => rustsp_quiet = true,
            "--rustsp-keep" => keep_generated = true,
            "--rustsp-clean" => clean_only = true,
            "--rustsp-reset" | "--reset" => reset_cache = true,
            "--rustsp-status" | "--status" => show_status = true,
            "-h" | "--help" if cargo_args.is_empty() => {
                print_usage();
                exit(0);
            }
            "-V" | "--version" if cargo_args.is_empty() => {
                println!("cargo-rustsp 1.0.0 (incremental, SHA-256 + Merkle tree)");
                exit(0);
            }
            _ => {
                // Filter out "clean" when preceded by --reset
                // to support "cargo rustsp --reset clean" syntax
                if arg == "clean" && reset_cache && cargo_args.is_empty() {
                    // "clean" is consumed as part of --reset clean
                    continue;
                }
                cargo_args.push(arg.clone());
            }
        }
    }

    // Find project root
    let project_root = match find_project_root() {
        Some(root) => root,
        None => {
            eprintln!(
                "{}error{}: could not find Cargo.toml in current directory or any parent",
                ansi::BOLD_RED,
                ansi::RESET
            );
            exit(1);
        }
    };

    // Handle --rustsp-status
    if show_status {
        print_status(&project_root);
        exit(0);
    }

    // Handle --rustsp-reset (or --reset clean)
    if reset_cache {
        let cache_dir = project_root.join("target").join("rustsp");

        if !rustsp_quiet {
            eprintln!(
                "{}   Resetting{} RustS+ incremental cache...",
                ansi::BOLD_CYAN,
                ansi::RESET
            );
        }

        // Remove entire target/rustsp/ directory
        if cache_dir.exists() {
            if let Err(e) = fs::remove_dir_all(&cache_dir) {
                eprintln!(
                    "{}error{}: failed to remove cache directory: {}",
                    ansi::BOLD_RED,
                    ansi::RESET,
                    e
                );
                exit(1);
            }
        }

        // Also clean any leftover deployed .rs files in source
        let cleaned = clean_deployed_files(&project_root);

        if !rustsp_quiet {
            eprintln!(
                "{}      Reset{} cache cleared (target/rustsp/ removed)",
                ansi::BOLD_GREEN,
                ansi::RESET
            );
            if cleaned > 0 {
                eprintln!(
                    "{}    Cleaned{} {} leftover .rs file(s) from source tree",
                    ansi::BOLD_GREEN,
                    ansi::RESET,
                    cleaned
                );
            }
            eprintln!(
                "{}      Ready{} next build will be a full compile",
                ansi::BOLD_GREEN,
                ansi::RESET
            );
        }

        exit(0);
    }

    // Handle --rustsp-clean (legacy: clean deployed .rs files only)
    if clean_only {
        if !rustsp_quiet {
            eprintln!(
                "{}    Cleaning{} any leftover deployed .rs files...",
                ansi::BOLD_CYAN,
                ansi::RESET
            );
        }
        let cleaned = clean_deployed_files(&project_root);
        if !rustsp_quiet {
            eprintln!(
                "{}    Cleaned{} {} file(s)",
                ansi::BOLD_GREEN,
                ansi::RESET,
                cleaned
            );
        }
        exit(0);
    }

    // If no cargo command provided, show usage
    if cargo_args.is_empty() {
        print_usage();
        exit(1);
    }

    // ========================================================================
    // INCREMENTAL COMPILATION
    // ========================================================================

    if !rustsp_quiet {
        eprintln!(
            "{}Preprocessing{} RustS+ files (incremental)...",
            ansi::BOLD_CYAN,
            ansi::RESET
        );
    }

    let mut compiler = IncrementalCompiler::new(project_root.clone(), rustsp_quiet, force_rebuild);

    let tracker = match compiler.process(keep_generated) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    };

    // ========================================================================
    // RUN CARGO
    // ========================================================================

    if !rustsp_quiet {
        eprintln!(
            "{}     Running{} cargo {}",
            ansi::BOLD_CYAN,
            ansi::RESET,
            cargo_args.join(" ")
        );
    }

    let cargo_result = Command::new("cargo")
        .current_dir(&project_root)
        .args(&cargo_args)
        .status();

    // Get exit code before tracker cleanup
    let exit_code = match cargo_result {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!(
                "{}error{}: failed to run cargo: {}",
                ansi::BOLD_RED,
                ansi::RESET,
                e
            );
            1
        }
    };

    // Tracker dropped here → auto-cleanup deployed .rs files
    drop(tracker);

    exit(exit_code);
}

// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_empty() {
        assert_eq!(
            sha256::hash_str(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_sha256_abc() {
        assert_eq!(
            sha256::hash_str("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn test_sha256_448bit() {
        // This tests the padding edge case (message length = 448 bits = 56 bytes)
        assert_eq!(
            sha256::hash_str("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn test_merkle_deterministic() {
        let paths1 = vec!["src/main.rss".to_string(), "src/lib.rss".to_string()];
        let paths2 = vec!["src/lib.rss".to_string(), "src/main.rss".to_string()];
        // Order shouldn't matter (sorted internally)
        assert_eq!(merkle::compute_root(&paths1), merkle::compute_root(&paths2));
    }

    #[test]
    fn test_merkle_structure_sensitivity() {
        let paths1 = vec!["src/main.rss".to_string(), "src/lib.rss".to_string()];
        let paths2 = vec!["src/main.rss".to_string()]; // missing lib.rss
        assert_ne!(merkle::compute_root(&paths1), merkle::compute_root(&paths2));
    }

    #[test]
    fn test_merkle_rename_sensitivity() {
        let paths1 = vec!["src/user.rss".to_string()];
        let paths2 = vec!["src/customer.rss".to_string()]; // renamed
        assert_ne!(merkle::compute_root(&paths1), merkle::compute_root(&paths2));
    }

    #[test]
    fn test_merkle_move_sensitivity() {
        let paths1 = vec!["src/models/user.rss".to_string()];
        let paths2 = vec!["src/entities/user.rss".to_string()]; // moved
        assert_ne!(merkle::compute_root(&paths1), merkle::compute_root(&paths2));
    }

    #[test]
    fn test_merkle_empty() {
        let paths: Vec<String> = vec![];
        let root = merkle::compute_root(&paths);
        assert!(!root.is_empty());
    }

    #[test]
    fn test_json_roundtrip() {
        let manifest = CompileManifest {
            version: "1.0.0".to_string(),
            merkle_root: "abc123".to_string(),
            last_updated: "1706745600".to_string(),
            files: {
                let mut m = BTreeMap::new();
                m.insert(
                    "src/main.rss".to_string(),
                    FileEntry {
                        source_path: "src/main.rss".to_string(),
                        content_hash: "deadbeef".to_string(),
                        file_name: "main.rss".to_string(),
                        cached_rs: "target/rustsp/src/main.rs".to_string(),
                        deploy_path: "src/main.rs".to_string(),
                    },
                );
                m
            },
        };

        // Create temp file
        let tmp = std::env::temp_dir().join("test_rustsp_manifest.json");
        manifest.save(&tmp).unwrap();

        // Reload
        let loaded = CompileManifest::load(&tmp).unwrap();
        assert_eq!(loaded.version, "1.0.0");
        assert_eq!(loaded.merkle_root, "abc123");
        assert_eq!(loaded.files.len(), 1);

        let entry = loaded.files.get("src/main.rss").unwrap();
        assert_eq!(entry.content_hash, "deadbeef");
        assert_eq!(entry.file_name, "main.rss");
        assert_eq!(entry.cached_rs, "target/rustsp/src/main.rs");
        assert_eq!(entry.deploy_path, "src/main.rs");

        let _ = fs::remove_file(&tmp);
    }

    #[test]
    fn test_change_detection_no_changes() {
        let mut manifest = CompileManifest::new();
        manifest.files.insert(
            "src/main.rss".to_string(),
            FileEntry {
                source_path: "src/main.rss".to_string(),
                content_hash: "aaa".to_string(),
                file_name: "main.rss".to_string(),
                cached_rs: "target/rustsp/src/main.rs".to_string(),
                deploy_path: "src/main.rs".to_string(),
            },
        );
        manifest.merkle_root = merkle::compute_root(&["src/main.rss".to_string()]);

        let mut current = BTreeMap::new();
        current.insert(
            "src/main.rss".to_string(),
            ScannedFile {
                relative_path: "src/main.rss".to_string(),
                content_hash: "aaa".to_string(),
                file_name: "main.rss".to_string(),
            },
        );

        let changes = detect_changes(&manifest, &current);
        assert!(!changes.structure_changed);
        assert_eq!(changes.changes.len(), 1);
        assert!(matches!(changes.changes[0].1, ChangeKind::Unchanged));
    }

    #[test]
    fn test_change_detection_modified() {
        let mut manifest = CompileManifest::new();
        manifest.files.insert(
            "src/main.rss".to_string(),
            FileEntry {
                source_path: "src/main.rss".to_string(),
                content_hash: "aaa".to_string(),
                file_name: "main.rss".to_string(),
                cached_rs: "target/rustsp/src/main.rs".to_string(),
                deploy_path: "src/main.rs".to_string(),
            },
        );
        manifest.merkle_root = merkle::compute_root(&["src/main.rss".to_string()]);

        let mut current = BTreeMap::new();
        current.insert(
            "src/main.rss".to_string(),
            ScannedFile {
                relative_path: "src/main.rss".to_string(),
                content_hash: "bbb".to_string(), // CHANGED
                file_name: "main.rss".to_string(),
            },
        );

        let changes = detect_changes(&manifest, &current);
        assert!(!changes.structure_changed); // same path → same structure
        assert_eq!(changes.changes.len(), 1);
        assert!(matches!(changes.changes[0].1, ChangeKind::Modified));
    }

    #[test]
    fn test_change_detection_rename() {
        let mut manifest = CompileManifest::new();
        manifest.files.insert(
            "src/user.rss".to_string(),
            FileEntry {
                source_path: "src/user.rss".to_string(),
                content_hash: "aaa".to_string(),
                file_name: "user.rss".to_string(),
                cached_rs: "target/rustsp/src/user.rs".to_string(),
                deploy_path: "src/user.rs".to_string(),
            },
        );
        manifest.merkle_root = merkle::compute_root(&["src/user.rss".to_string()]);

        let mut current = BTreeMap::new();
        current.insert(
            "src/customer.rss".to_string(), // RENAMED
            ScannedFile {
                relative_path: "src/customer.rss".to_string(),
                content_hash: "aaa".to_string(), // Same content hash
                file_name: "customer.rss".to_string(),
            },
        );

        let changes = detect_changes(&manifest, &current);
        assert!(changes.structure_changed);

        // Should detect rename (not new + delete)
        let has_rename = changes
            .changes
            .iter()
            .any(|(_, c)| matches!(c, ChangeKind::Renamed { .. }));
        assert!(has_rename, "Should detect rename, got: {:?}", 
            changes.changes.iter().map(|(p, c)| (p, c.display())).collect::<Vec<_>>());
    }

    #[test]
    fn test_change_detection_new_and_deleted() {
        let mut manifest = CompileManifest::new();
        manifest.files.insert(
            "src/old.rss".to_string(),
            FileEntry {
                source_path: "src/old.rss".to_string(),
                content_hash: "aaa".to_string(),
                file_name: "old.rss".to_string(),
                cached_rs: "target/rustsp/src/old.rs".to_string(),
                deploy_path: "src/old.rs".to_string(),
            },
        );
        manifest.merkle_root = merkle::compute_root(&["src/old.rss".to_string()]);

        let mut current = BTreeMap::new();
        current.insert(
            "src/new.rss".to_string(), // Different content hash → truly new
            ScannedFile {
                relative_path: "src/new.rss".to_string(),
                content_hash: "bbb".to_string(), // Different hash
                file_name: "new.rss".to_string(),
            },
        );

        let changes = detect_changes(&manifest, &current);
        assert!(changes.structure_changed);

        let has_new = changes
            .changes
            .iter()
            .any(|(_, c)| matches!(c, ChangeKind::New));
        let has_deleted = changes
            .changes
            .iter()
            .any(|(_, c)| matches!(c, ChangeKind::Deleted));

        assert!(has_new);
        assert!(has_deleted);
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("src\\main.rss"), "src/main.rss");
        assert_eq!(normalize_path("src/main.rss"), "src/main.rss");
    }

    #[test]
    fn test_deploy_tracker_cleanup() {
        use std::io::Write;

        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_rustsp_deploy_cleanup.rs");

        // Write something
        {
            let mut f = fs::File::create(&test_file).unwrap();
            f.write_all(b"// test").unwrap();
        }

        // Create tracker and add the file as deployed
        {
            let mut tracker = DeployTracker::new(true, false);
            tracker.deployed.push(test_file.clone());
            assert!(test_file.exists());
            // tracker dropped here → cleanup
        }

        assert!(!test_file.exists());
    }
}