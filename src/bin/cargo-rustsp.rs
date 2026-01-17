//! cargo-rustsp v1.0.0 - Transparent Cargo Wrapper for RustS+
//!
//! Simple design:
//! 1. Find project root (Cargo.toml)
//! 2. Preprocess all .rss files → .rs (in-place)
//! 3. Pass ALL arguments to cargo unchanged
//!
//! This means ANY cargo command works:
//!   cargo rustsp build --workspace --release
//!   cargo rustsp test --test foo -- --ignored --nocapture
//!   cargo rustsp run -p my-crate --bin my-bin -- arg1 arg2
//!   cargo rustsp doc --no-deps
//!   cargo rustsp clippy -- -W warnings
//!   etc.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

//=============================================================================
// ANSI Colors
//=============================================================================

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
}

//=============================================================================
// Build Cache
//=============================================================================

#[derive(Debug)]
struct FileCache {
    entries: HashMap<PathBuf, u64>, // source path -> content hash
    cache_file: PathBuf,
}

impl FileCache {
    fn new(cache_dir: &Path) -> Self {
        let cache_file = cache_dir.join(".rustsp_cache");
        let entries = Self::load(&cache_file).unwrap_or_default();
        FileCache { entries, cache_file }
    }
    
    fn load(path: &Path) -> Option<HashMap<PathBuf, u64>> {
        let content = fs::read_to_string(path).ok()?;
        let mut entries = HashMap::new();
        for line in content.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                if let Ok(hash) = parts[1].parse() {
                    entries.insert(PathBuf::from(parts[0]), hash);
                }
            }
        }
        Some(entries)
    }
    
    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.cache_file.parent() {
            fs::create_dir_all(parent)?;
        }
        let content: String = self.entries.iter()
            .map(|(p, h)| format!("{}\t{}", p.display(), h))
            .collect::<Vec<_>>()
            .join("\n");
        fs::write(&self.cache_file, content)
    }
    
    fn needs_rebuild(&self, source: &Path, hash: u64) -> bool {
        self.entries.get(source).map(|&h| h != hash).unwrap_or(true)
    }
    
    fn update(&mut self, source: PathBuf, hash: u64) {
        self.entries.insert(source, hash);
    }
}

fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

//=============================================================================
// Core Functions
//=============================================================================

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
    // Check common names
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

/// Find all .rss files in a directory recursively
fn find_rss_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    find_rss_recursive(dir, &mut files);
    files
}

fn find_rss_recursive(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            // Skip target directory
            if path.file_name().map(|n| n != "target").unwrap_or(true) {
                find_rss_recursive(&path, files);
            }
        } else if path.extension().map(|e| e == "rss").unwrap_or(false) {
            files.push(path);
        }
    }
}

/// Compile a single .rss file to .rs
fn compile_rss(rustsp: &str, input: &Path, output: &Path, quiet: bool) -> Result<(), String> {
    let result = Command::new(rustsp)
        .arg(input)
        .arg("--emit-rs")
        .arg("-o")
        .arg(output)
        .output()
        .map_err(|e| format!("Failed to run {}: {}", rustsp, e))?;
    
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        if !quiet {
            if !stderr.is_empty() { eprintln!("{}", stderr); }
            if !stdout.is_empty() { eprintln!("{}", stdout); }
        }
        return Err(format!("Compilation failed: {}", input.display()));
    }
    
    if !output.exists() {
        return Err(format!("Compiler did not create output: {}", output.display()));
    }
    
    Ok(())
}

/// Preprocess all .rss files in the project IN-PLACE
fn preprocess_project(project_root: &Path, force: bool, quiet: bool) -> Result<(usize, usize), String> {
    let cache_dir = project_root.join("target").join("rustsp_cache");
    let mut cache = FileCache::new(&cache_dir);
    let rustsp = find_rustsp_binary();
    
    // Find all .rss files
    let rss_files = find_rss_files(project_root);
    
    if rss_files.is_empty() {
        return Ok((0, 0));
    }
    
    let mut compiled = 0;
    let mut cached = 0;
    
    for rss_path in &rss_files {
        let rs_path = rss_path.with_extension("rs");
        
        // Read and hash content
        let content = fs::read_to_string(rss_path)
            .map_err(|e| format!("Failed to read {}: {}", rss_path.display(), e))?;
        let content_hash = hash_content(&content);
        
        // Check if rebuild needed
        let needs_rebuild = force || 
                           cache.needs_rebuild(rss_path, content_hash) ||
                           !rs_path.exists();
        
        if needs_rebuild {
            if !quiet {
                let display_path = rss_path.strip_prefix(project_root)
                    .unwrap_or(rss_path);
                eprintln!("  {}{}{} {}", ansi::DIM, "Compiling", ansi::RESET, display_path.display());
            }
            
            compile_rss(&rustsp, rss_path, &rs_path, quiet)?;
            cache.update(rss_path.clone(), content_hash);
            compiled += 1;
        } else {
            cached += 1;
        }
    }
    
    // Save cache
    let _ = cache.save();
    
    Ok((compiled, cached))
}

/// Clean generated .rs files (those with corresponding .rss)
fn clean_generated_files(project_root: &Path) -> usize {
    let mut cleaned = 0;
    
    fn clean_recursive(dir: &Path, cleaned: &mut usize) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                if path.file_name().map(|n| n != "target").unwrap_or(true) {
                    clean_recursive(&path, cleaned);
                }
            } else if path.extension().map(|e| e == "rs").unwrap_or(false) {
                // Check if there's a corresponding .rss file
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

//=============================================================================
// Main
//=============================================================================

fn print_usage() {
    eprintln!("{}cargo-rustsp{} v1.0.0 - Transparent Cargo Wrapper for RustS+", 
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!();
    eprintln!("{}USAGE:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    cargo rustsp [CARGO_COMMAND] [ARGS...]");
    eprintln!();
    eprintln!("{}HOW IT WORKS:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    1. Finds all .rss files in the project");
    eprintln!("    2. Compiles them to .rs (in-place, alongside source)");
    eprintln!("    3. Passes ALL arguments to cargo unchanged");
    eprintln!();
    eprintln!("{}EXAMPLES:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    cargo rustsp build");
    eprintln!("    cargo rustsp build --workspace --release");
    eprintln!("    cargo rustsp test --test my_test -- --ignored --nocapture");
    eprintln!("    cargo rustsp run -p my-crate --bin my-bin -- arg1 arg2");
    eprintln!("    cargo rustsp run -p dsdn-coordinator");
    eprintln!("    cargo rustsp run -p dsdn-node --bin dsdn-node -- auto zone-a 50051");
    eprintln!("    cargo rustsp doc --no-deps");
    eprintln!("    cargo rustsp clippy -- -W warnings");
    eprintln!("    cargo rustsp check");
    eprintln!("    cargo rustsp bench");
    eprintln!();
    eprintln!("{}RUSTSP-SPECIFIC OPTIONS:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    --rustsp-force    Force rebuild all .rss files (ignore cache)");
    eprintln!("    --rustsp-quiet    Suppress rustsp preprocessing output");
    eprintln!("    --rustsp-clean    Clean generated .rs files and exit");
    eprintln!();
    eprintln!("{}NOTE:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    Any cargo command works! cargo-rustsp just preprocesses .rss");
    eprintln!("    files before running cargo with your exact arguments.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Skip "cargo" if invoked as "cargo rustsp"
    let start_idx = if args.len() > 1 && args[1] == "rustsp" { 2 } else { 1 };
    
    // Extract rustsp-specific flags
    let mut force_rebuild = false;
    let mut rustsp_quiet = false;
    let mut clean_only = false;
    let mut cargo_args: Vec<String> = Vec::new();
    
    for arg in args.iter().skip(start_idx) {
        match arg.as_str() {
            "--rustsp-force" => force_rebuild = true,
            "--rustsp-quiet" => rustsp_quiet = true,
            "--rustsp-clean" => clean_only = true,
            "-h" | "--help" if cargo_args.is_empty() => {
                print_usage();
                exit(0);
            }
            "-V" | "--version" if cargo_args.is_empty() => {
                println!("cargo-rustsp 2.0.0");
                exit(0);
            }
            _ => cargo_args.push(arg.clone()),
        }
    }
    
    // Find project root
    let project_root = match find_project_root() {
        Some(root) => root,
        None => {
            eprintln!("{}error{}: could not find Cargo.toml in current directory or any parent",
                ansi::BOLD_RED, ansi::RESET);
            exit(1);
        }
    };
    
    // Handle clean-only
    if clean_only {
        if !rustsp_quiet {
            eprintln!("{}   Cleaning{} generated .rs files...", ansi::BOLD_CYAN, ansi::RESET);
        }
        let cleaned = clean_generated_files(&project_root);
        
        // Also clean cache
        let cache_dir = project_root.join("target").join("rustsp_cache");
        if cache_dir.exists() {
            let _ = fs::remove_dir_all(&cache_dir);
        }
        
        if !rustsp_quiet {
            eprintln!("{}   Cleaned{} {} generated .rs file(s)", 
                ansi::BOLD_GREEN, ansi::RESET, cleaned);
        }
        exit(0);
    }
    
    // If no cargo command provided, show usage
    if cargo_args.is_empty() {
        print_usage();
        exit(1);
    }
    
    // Preprocess .rss files
    if !rustsp_quiet {
        eprintln!("{}  Preprocessing{} RustS+ files...", ansi::BOLD_CYAN, ansi::RESET);
    }
    
    match preprocess_project(&project_root, force_rebuild, rustsp_quiet) {
        Ok((compiled, cached)) => {
            if !rustsp_quiet && (compiled > 0 || cached > 0) {
                eprintln!("{}   Preprocessed{} {} compiled, {} cached", 
                    ansi::BOLD_GREEN, ansi::RESET, compiled, cached);
            } else if !rustsp_quiet && compiled == 0 && cached == 0 {
                eprintln!("  {}(no .rss files found){}", ansi::DIM, ansi::RESET);
            }
        }
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    }
    
    // Run cargo with all provided arguments
    if !rustsp_quiet {
        eprintln!("{}      Running{} cargo {}", 
            ansi::BOLD_CYAN, ansi::RESET, 
            cargo_args.join(" "));
    }
    
    let status = Command::new("cargo")
        .current_dir(&project_root)
        .args(&cargo_args)
        .status();
    
    match status {
        Ok(s) => {
            if !s.success() {
                exit(s.code().unwrap_or(1));
            }
        }
        Err(e) => {
            eprintln!("{}error{}: failed to run cargo: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    }
}

//=============================================================================
// Tests
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_hash_content() {
        let h1 = hash_content("hello world");
        let h2 = hash_content("hello world");
        let h3 = hash_content("hello world!");
        
        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }
}