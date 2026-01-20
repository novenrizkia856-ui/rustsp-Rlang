//! cargo-rustsp v1.0.0 - Transparent Cargo Wrapper for RustS+
//!
//! Design:
//! 1. Find project root (Cargo.toml)
//! 2. Preprocess all .rss files → .rs (in-place)
//! 3. Track all generated .rs files
//! 4. Pass ALL arguments to cargo unchanged
//! 5. AUTO-CLEANUP: Delete generated .rs files after cargo finishes
//!
//! This keeps your source tree clean - only .rss files remain!

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
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
}

//=============================================================================
// Generated Files Tracker - Auto cleanup on drop
//=============================================================================

/// Tracks generated .rs files and cleans them up when dropped
struct GeneratedFilesTracker {
    files: Vec<PathBuf>,
    quiet: bool,
    keep_generated: bool,
}

impl GeneratedFilesTracker {
    fn new(quiet: bool, keep_generated: bool) -> Self {
        GeneratedFilesTracker {
            files: Vec::new(),
            quiet,
            keep_generated,
        }
    }
    
    fn track(&mut self, path: PathBuf) {
        self.files.push(path);
    }
    
    fn cleanup(&mut self) {
        if self.keep_generated {
            if !self.quiet && !self.files.is_empty() {
                eprintln!("{}      Keeping{} {} generated .rs file(s) (--rustsp-keep)", 
                    ansi::BOLD_YELLOW, ansi::RESET, self.files.len());
            }
            return;
        }
        
        let mut cleaned = 0;
        for path in &self.files {
            if path.exists() {
                if fs::remove_file(path).is_ok() {
                    cleaned += 1;
                }
            }
        }
        
        if !self.quiet && cleaned > 0 {
            eprintln!("{}     Cleanup{} removed {} generated .rs file(s)", 
                ansi::BOLD_GREEN, ansi::RESET, cleaned);
        }
        
        self.files.clear();
    }
    
    fn file_count(&self) -> usize {
        self.files.len()
    }
}

impl Drop for GeneratedFilesTracker {
    fn drop(&mut self) {
        // Auto cleanup when tracker goes out of scope
        // This handles normal exit, early return, and panic
        self.cleanup();
    }
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
fn compile_rss(rustsp: &str, input: &Path, output: &Path, _quiet: bool) -> Result<(), String> {
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
        // CRITICAL FIX: ALWAYS print errors, even in quiet mode
        // Errors should never be silenced - only progress messages should be quiet
        if !stderr.is_empty() { eprintln!("{}", stderr); }
        if !stdout.is_empty() { eprintln!("{}", stdout); }
        return Err(format!("Compilation failed: {}", input.display()));
    }
    
    if !output.exists() {
        return Err(format!("Compiler did not create output: {}", output.display()));
    }
    
    Ok(())
}

/// Preprocess all .rss files in the project IN-PLACE
/// Returns the tracker that will auto-cleanup on drop
/// 
/// CRITICAL FIX: Process ALL files and collect ALL errors before stopping.
/// This ensures developers see all RustS+ errors at once, not just the first one.
fn preprocess_project(
    project_root: &Path, 
    force: bool, 
    quiet: bool,
    keep_generated: bool,
) -> Result<GeneratedFilesTracker, String> {
    let cache_dir = project_root.join("target").join("rustsp_cache");
    let mut cache = FileCache::new(&cache_dir);
    let rustsp = find_rustsp_binary();
    
    // Create tracker for generated files
    let mut tracker = GeneratedFilesTracker::new(quiet, keep_generated);
    
    // Find all .rss files
    let rss_files = find_rss_files(project_root);
    
    if rss_files.is_empty() {
        if !quiet {
            eprintln!("  {}(no .rss files found){}", ansi::DIM, ansi::RESET);
        }
        return Ok(tracker);
    }
    
    let mut compiled = 0;
    let mut cached = 0;
    
    // CRITICAL FIX: Collect ALL errors instead of stopping on first error
    let mut all_errors: Vec<(PathBuf, String)> = Vec::new();
    let mut failed_files: Vec<PathBuf> = Vec::new();
    
    for rss_path in &rss_files {
        let rs_path = rss_path.with_extension("rs");
        
        // Read and hash content
        let content = match fs::read_to_string(rss_path) {
            Ok(c) => c,
            Err(e) => {
                all_errors.push((rss_path.clone(), format!("Failed to read: {}", e)));
                continue;
            }
        };
        let content_hash = hash_content(&content);
        
        // CRITICAL FIX: ALWAYS recompile if content changed, even if .rs exists
        // The old .rs might be stale (from before RustS+ errors were introduced)
        let needs_compile = force || 
                           cache.needs_rebuild(rss_path, content_hash) ||
                           !rs_path.exists();
        
        if needs_compile {
            if !quiet {
                let display_path = rss_path.strip_prefix(project_root)
                    .unwrap_or(rss_path);
                eprintln!("   {}Compiling{} {}", ansi::DIM, ansi::RESET, display_path.display());
            }
            
            // CRITICAL FIX: Don't use ? operator - collect error and continue
            match compile_rss(&rustsp, rss_path, &rs_path, quiet) {
                Ok(()) => {
                    cache.update(rss_path.clone(), content_hash);
                    compiled += 1;
                    tracker.track(rs_path);
                }
                Err(e) => {
                    all_errors.push((rss_path.clone(), e));
                    failed_files.push(rss_path.clone());
                    // DON'T track failed files - no .rs was generated
                    // DON'T update cache - file needs recompile next time
                    continue;
                }
            }
        } else {
            // CRITICAL FIX: Even for cached files, verify the .rs file exists
            // If it doesn't (e.g., manual deletion), force recompile
            if !rs_path.exists() {
                if !quiet {
                    let display_path = rss_path.strip_prefix(project_root)
                        .unwrap_or(rss_path);
                    eprintln!("   {}Compiling{} {} (cache miss)", ansi::DIM, ansi::RESET, display_path.display());
                }
                
                match compile_rss(&rustsp, rss_path, &rs_path, quiet) {
                    Ok(()) => {
                        compiled += 1;
                        tracker.track(rs_path);
                    }
                    Err(e) => {
                        all_errors.push((rss_path.clone(), e));
                        failed_files.push(rss_path.clone());
                        continue;
                    }
                }
            } else {
                cached += 1;
                tracker.track(rs_path);
            }
        }
    }
    
    // Save cache (only for successful compiles)
    let _ = cache.save();
    
    // CRITICAL FIX: If ANY errors occurred, show ALL of them and fail
    if !all_errors.is_empty() {
        eprintln!("\n{}╔═══════════════════════════════════════════════════════════════╗{}", 
            ansi::BOLD_RED, ansi::RESET);
        eprintln!("{}║   RUSTS+ COMPILATION ERRORS ({} file(s) failed)               ║{}",
            ansi::BOLD_RED, all_errors.len(), ansi::RESET);
        eprintln!("{}╚═══════════════════════════════════════════════════════════════╝{}\n",
            ansi::BOLD_RED, ansi::RESET);
        
        // Errors were already printed by compile_rss, just show summary
        eprintln!("{}Failed files:{}", ansi::BOLD_RED, ansi::RESET);
        for path in &failed_files {
            let display_path = path.strip_prefix(project_root).unwrap_or(path);
            eprintln!("  • {}", display_path.display());
        }
        eprintln!();
        
        // Clean up any .rs files that WERE generated before failing
        // This prevents partial compilation
        tracker.cleanup();
        
        return Err(format!(
            "{} RustS+ file(s) failed to compile. Fix all errors above and try again.",
            all_errors.len()
        ));
    }
    
    if !quiet {
        eprintln!("{}  Preprocessed{} {} compiled, {} cached", 
            ansi::BOLD_GREEN, ansi::RESET, compiled, cached);
    }
    
    Ok(tracker)
}

/// Manually clean all generated .rs files (those with corresponding .rss)
fn clean_all_generated_files(project_root: &Path) -> usize {
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
    eprintln!("    2. Compiles them to .rs (in-place, temporarily)");
    eprintln!("    3. Runs cargo with your exact arguments");
    eprintln!("    4. {}AUTO-CLEANS{} generated .rs files after cargo finishes", ansi::BOLD_GREEN, ansi::RESET);
    eprintln!();
    eprintln!("    Your source tree stays clean - only .rss files remain!");
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
    eprintln!("    --rustsp-force    Force recompile all .rss files (ignore cache)");
    eprintln!("    --rustsp-quiet    Suppress rustsp preprocessing output");
    eprintln!("    --rustsp-keep     Keep generated .rs files (don't auto-clean)");
    eprintln!("    --rustsp-clean    Manually clean any leftover .rs files and exit");
    eprintln!();
    eprintln!("{}NOTE:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    Any cargo command works! cargo-rustsp is a transparent wrapper.");
}

fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Skip "cargo" if invoked as "cargo rustsp"
    let start_idx = if args.len() > 1 && args[1] == "rustsp" { 2 } else { 1 };
    
    // Extract rustsp-specific flags
    let mut force_rebuild = false;
    let mut rustsp_quiet = false;
    let mut keep_generated = false;
    let mut clean_only = false;
    let mut cargo_args: Vec<String> = Vec::new();
    
    for arg in args.iter().skip(start_idx) {
        match arg.as_str() {
            "--rustsp-force" => force_rebuild = true,
            "--rustsp-quiet" => rustsp_quiet = true,
            "--rustsp-keep" => keep_generated = true,
            "--rustsp-clean" => clean_only = true,
            "-h" | "--help" if cargo_args.is_empty() => {
                print_usage();
                exit(0);
            }
            "-V" | "--version" if cargo_args.is_empty() => {
                println!("cargo-rustsp 2.1.0");
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
    
    // Handle manual clean
    if clean_only {
        if !rustsp_quiet {
            eprintln!("{}    Cleaning{} any leftover generated .rs files...", ansi::BOLD_CYAN, ansi::RESET);
        }
        let cleaned = clean_all_generated_files(&project_root);
        
        // Also clean cache
        let cache_dir = project_root.join("target").join("rustsp_cache");
        if cache_dir.exists() {
            let _ = fs::remove_dir_all(&cache_dir);
        }
        
        if !rustsp_quiet {
            eprintln!("{}    Cleaned{} {} file(s)", 
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
        eprintln!("{}Preprocessing{} RustS+ files...", ansi::BOLD_CYAN, ansi::RESET);
    }
    
    // The tracker will auto-cleanup when it goes out of scope
    let tracker = match preprocess_project(&project_root, force_rebuild, rustsp_quiet, keep_generated) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    };
    
    // Run cargo with all provided arguments
    if !rustsp_quiet {
        eprintln!("{}     Running{} cargo {}", 
            ansi::BOLD_CYAN, ansi::RESET, 
            cargo_args.join(" "));
    }
    
    let cargo_result = Command::new("cargo")
        .current_dir(&project_root)
        .args(&cargo_args)
        .status();
    
    // Get exit code before tracker cleanup
    let exit_code = match cargo_result {
        Ok(status) => status.code().unwrap_or(1),
        Err(e) => {
            eprintln!("{}error{}: failed to run cargo: {}", ansi::BOLD_RED, ansi::RESET, e);
            1
        }
    };
    
    // Tracker will be dropped here and cleanup will happen automatically
    // This is explicit drop to show when cleanup happens
    drop(tracker);
    
    exit(exit_code);
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
    
    #[test]
    fn test_tracker_cleanup() {
        use std::io::Write;
        
        // Create temp file
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_rustsp_cleanup.rs");
        
        // Write something
        {
            let mut f = fs::File::create(&test_file).unwrap();
            f.write_all(b"// test").unwrap();
        }
        
        // Create tracker and track the file
        {
            let mut tracker = GeneratedFilesTracker::new(true, false);
            tracker.track(test_file.clone());
            assert!(test_file.exists());
            // tracker dropped here, should cleanup
        }
        
        // File should be deleted
        assert!(!test_file.exists());
    }
}