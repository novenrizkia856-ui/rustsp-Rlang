//! cargo-rustsp - Robust Cargo Integration for RustS+
//! v0.9.0 - Full multi-module, workspace, and incremental compilation support
//!
//! Features:
//! - Multi-module resolution (nested modules, mod.rss)
//! - Workspace support (multiple crates)
//! - Incremental compilation (hash-based caching)
//! - Proper module graph building
//! - Enhanced error reporting
//! - Mixed .rs/.rss projects

use std::collections::{HashMap, HashSet, BTreeMap};
use std::env;
use std::fs::{self, File};
use std::io::{self, Read, Write, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, exit, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;

//=============================================================================
// ANSI Color Codes
//=============================================================================

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD: &str = "\x1b[1m";
    pub const DIM: &str = "\x1b[2m";
    
    pub const RED: &str = "\x1b[31m";
    pub const GREEN: &str = "\x1b[32m";
    pub const YELLOW: &str = "\x1b[33m";
    pub const BLUE: &str = "\x1b[34m";
    pub const MAGENTA: &str = "\x1b[35m";
    pub const CYAN: &str = "\x1b[36m";
    
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const BOLD_YELLOW: &str = "\x1b[1;33m";
    pub const BOLD_BLUE: &str = "\x1b[1;34m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
}

//=============================================================================
// Build Cache for Incremental Compilation
//=============================================================================

#[derive(Debug, Clone)]
struct FileCache {
    entries: HashMap<PathBuf, CacheEntry>,
    cache_file: PathBuf,
}

#[derive(Debug, Clone)]
struct CacheEntry {
    source_hash: u64,
    modified_time: u64,
    output_path: PathBuf,
}

impl FileCache {
    fn new(cache_dir: &Path) -> Self {
        let cache_file = cache_dir.join(".rustsp_cache");
        let entries = Self::load_cache(&cache_file).unwrap_or_default();
        FileCache { entries, cache_file }
    }
    
    fn load_cache(path: &Path) -> Option<HashMap<PathBuf, CacheEntry>> {
        let content = fs::read_to_string(path).ok()?;
        let mut entries = HashMap::new();
        
        for line in content.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 4 {
                let source = PathBuf::from(parts[0]);
                let hash: u64 = parts[1].parse().ok()?;
                let mtime: u64 = parts[2].parse().ok()?;
                let output = PathBuf::from(parts[3]);
                
                entries.insert(source, CacheEntry {
                    source_hash: hash,
                    modified_time: mtime,
                    output_path: output,
                });
            }
        }
        Some(entries)
    }
    
    fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.cache_file.parent() {
            fs::create_dir_all(parent)?;
        }
        
        let mut content = String::new();
        for (source, entry) in &self.entries {
            content.push_str(&format!(
                "{}\t{}\t{}\t{}\n",
                source.display(),
                entry.source_hash,
                entry.modified_time,
                entry.output_path.display()
            ));
        }
        fs::write(&self.cache_file, content)
    }
    
    fn needs_rebuild(&self, source: &Path, content_hash: u64) -> bool {
        match self.entries.get(source) {
            Some(entry) => entry.source_hash != content_hash,
            None => true,
        }
    }
    
    fn update(&mut self, source: PathBuf, hash: u64, output: PathBuf) {
        let mtime = get_modified_time(&source);
        self.entries.insert(source, CacheEntry {
            source_hash: hash,
            modified_time: mtime,
            output_path: output,
        });
    }
}

fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

fn get_modified_time(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .and_then(|t| t.duration_since(UNIX_EPOCH).map(|d| d.as_secs()).map_err(|e| io::Error::new(io::ErrorKind::Other, e)))
        .unwrap_or(0)
}

//=============================================================================
// Module Graph - Tracks module dependencies and structure
//=============================================================================

#[derive(Debug, Clone)]
struct ModuleNode {
    /// Original source file path (.rss or .rs)
    source_path: PathBuf,
    /// Module name (e.g., "parser", "parser::lexer")
    module_name: String,
    /// Is this a .rss file that needs compilation?
    is_rustsp: bool,
    /// Child modules declared via `mod foo;`
    children: Vec<String>,
    /// Output path in shadow directory
    output_path: PathBuf,
}

#[derive(Debug, Clone)]
struct ModuleGraph {
    /// All modules indexed by their path relative to src/
    nodes: BTreeMap<String, ModuleNode>,
    /// Root modules (main.rs/lib.rs)
    roots: Vec<String>,
}

impl ModuleGraph {
    fn new() -> Self {
        ModuleGraph {
            nodes: BTreeMap::new(),
            roots: Vec::new(),
        }
    }
    
    fn add_node(&mut self, rel_path: String, node: ModuleNode) {
        self.nodes.insert(rel_path, node);
    }
    
    fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
    
    fn rss_count(&self) -> usize {
        self.nodes.values().filter(|n| n.is_rustsp).count()
    }
    
    fn rs_count(&self) -> usize {
        self.nodes.values().filter(|n| !n.is_rustsp).count()
    }
}

//=============================================================================
// Module Resolution - Handles Rust's module system
//=============================================================================

/// Parse a file to extract `mod` declarations
fn extract_mod_declarations(content: &str) -> Vec<(String, Option<String>)> {
    let mut mods = Vec::new();
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        // Skip comments
        if trimmed.starts_with("//") || trimmed.starts_with("/*") {
            continue;
        }
        
        // Match: mod foo; or pub mod foo; or #[path = "..."] mod foo;
        let line_without_pub = if trimmed.starts_with("pub ") {
            &trimmed[4..]
        } else {
            trimmed
        };
        
        // Check for #[path = "..."] attribute on previous lines
        // For simplicity, we'll handle inline path attributes
        let path_attr = extract_path_attribute(trimmed);
        
        if line_without_pub.starts_with("mod ") {
            // Extract module name
            let rest = line_without_pub[4..].trim();
            
            // mod foo; (declaration) vs mod foo { } (inline)
            if let Some(semicolon) = rest.find(';') {
                let mod_name = rest[..semicolon].trim().to_string();
                if is_valid_module_name(&mod_name) {
                    mods.push((mod_name, path_attr));
                }
            }
            // Inline modules (mod foo { ... }) are not external files
        }
    }
    
    mods
}

fn extract_path_attribute(line: &str) -> Option<String> {
    // Simple extraction of #[path = "..."]
    if line.contains("#[path") {
        if let Some(start) = line.find('"') {
            if let Some(end) = line[start+1..].find('"') {
                return Some(line[start+1..start+1+end].to_string());
            }
        }
    }
    None
}

fn is_valid_module_name(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let first = name.chars().next().unwrap();
    if !first.is_ascii_lowercase() && first != '_' {
        return false;
    }
    name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Resolve module path following Rust's module resolution rules
/// Returns (source_path, is_rustsp)
fn resolve_module_path(
    parent_dir: &Path,
    mod_name: &str,
    custom_path: Option<&str>,
) -> Option<(PathBuf, bool)> {
    // If custom path specified, use it
    if let Some(path) = custom_path {
        let full_path = parent_dir.join(path);
        if full_path.exists() {
            let is_rss = path.ends_with(".rss");
            return Some((full_path, is_rss));
        }
    }
    
    // Standard resolution order:
    // 1. mod_name.rss (RustS+ file)
    // 2. mod_name/mod.rss (RustS+ directory module)
    // 3. mod_name.rs (Rust file)
    // 4. mod_name/mod.rs (Rust directory module)
    
    let candidates = [
        (parent_dir.join(format!("{}.rss", mod_name)), true),
        (parent_dir.join(mod_name).join("mod.rss"), true),
        (parent_dir.join(format!("{}.rs", mod_name)), false),
        (parent_dir.join(mod_name).join("mod.rs"), false),
    ];
    
    for (path, is_rss) in candidates {
        if path.exists() {
            return Some((path, is_rss));
        }
    }
    
    None
}

/// Build complete module graph starting from entry points
fn build_module_graph(
    src_dir: &Path,
    shadow_src: &Path,
) -> Result<ModuleGraph, String> {
    let mut graph = ModuleGraph::new();
    let mut to_process: Vec<(PathBuf, String, PathBuf)> = Vec::new(); // (source, module_path, output)
    
    // Find root modules
    let roots = [
        ("main.rss", "main", true),
        ("main.rs", "main", false),
        ("lib.rss", "lib", true),
        ("lib.rs", "lib", false),
    ];
    
    for (filename, mod_name, is_rss) in roots {
        let source_path = src_dir.join(filename);
        if source_path.exists() {
            let output_name = if is_rss { 
                format!("{}.rs", mod_name) 
            } else { 
                filename.to_string() 
            };
            let output_path = shadow_src.join(&output_name);
            
            graph.roots.push(mod_name.to_string());
            to_process.push((source_path, mod_name.to_string(), output_path));
        }
    }
    
    // Process modules recursively
    let mut processed: HashSet<PathBuf> = HashSet::new();
    
    while let Some((source_path, module_path, output_path)) = to_process.pop() {
        if processed.contains(&source_path) {
            continue;
        }
        processed.insert(source_path.clone());
        
        let is_rustsp = source_path.extension()
            .map(|e| e == "rss")
            .unwrap_or(false);
        
        // Read file content
        let content = fs::read_to_string(&source_path)
            .map_err(|e| format!("Failed to read {}: {}", source_path.display(), e))?;
        
        // Extract mod declarations
        let mods = extract_mod_declarations(&content);
        let mut children = Vec::new();
        
        // Determine parent directory for resolving child modules
        let parent_dir = if source_path.file_name().map(|n| n == "mod.rss" || n == "mod.rs").unwrap_or(false) {
            source_path.parent().unwrap().to_path_buf()
        } else {
            // For main.rss/lib.rss, look for modules in same directory
            // For foo.rss, look in foo/ directory
            let stem = source_path.file_stem().unwrap().to_string_lossy();
            if stem == "main" || stem == "lib" {
                source_path.parent().unwrap().to_path_buf()
            } else {
                // foo.rss -> look in foo/
                source_path.parent().unwrap().join(stem.as_ref())
            }
        };
        
        // Also check sibling directory for non-mod files
        let sibling_check_dir = source_path.parent().unwrap();
        
        for (mod_name, custom_path) in mods {
            children.push(mod_name.clone());
            
            // Try parent_dir first, then sibling check dir
            let resolved = resolve_module_path(&parent_dir, &mod_name, custom_path.as_deref())
                .or_else(|| resolve_module_path(sibling_check_dir, &mod_name, custom_path.as_deref()));
            
            if let Some((child_source, child_is_rss)) = resolved {
                let child_module_path = if module_path == "main" || module_path == "lib" {
                    mod_name.clone()
                } else {
                    format!("{}::{}", module_path, mod_name)
                };
                
                // Calculate output path
                let rel_source = child_source.strip_prefix(src_dir)
                    .unwrap_or(&child_source);
                let output_rel = if child_is_rss {
                    rel_source.with_extension("rs")
                } else {
                    rel_source.to_path_buf()
                };
                let child_output = shadow_src.join(output_rel);
                
                to_process.push((child_source, child_module_path, child_output));
            }
        }
        
        // Calculate relative path for graph key
        let rel_path = source_path.strip_prefix(src_dir)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| source_path.to_string_lossy().to_string());
        
        graph.add_node(rel_path, ModuleNode {
            source_path,
            module_name: module_path,
            is_rustsp,
            children,
            output_path,
        });
    }
    
    Ok(graph)
}

//=============================================================================
// Workspace Support
//=============================================================================

#[derive(Debug, Clone)]
struct WorkspaceMember {
    name: String,
    path: PathBuf,
    has_rustsp: bool,
}

#[derive(Debug, Clone)]
enum ProjectKind {
    SingleCrate,
    Workspace(Vec<WorkspaceMember>),
}

fn detect_project_kind(root: &Path) -> Result<ProjectKind, String> {
    let cargo_toml = root.join("Cargo.toml");
    let content = fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    
    // Check for workspace section
    if content.contains("[workspace]") {
        let members = parse_workspace_members(&content, root)?;
        if members.is_empty() {
            return Err("Workspace has no members".to_string());
        }
        return Ok(ProjectKind::Workspace(members));
    }
    
    Ok(ProjectKind::SingleCrate)
}

fn parse_workspace_members(content: &str, root: &Path) -> Result<Vec<WorkspaceMember>, String> {
    let mut members = Vec::new();
    let mut in_workspace = false;
    let mut in_members = false;
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        if trimmed == "[workspace]" {
            in_workspace = true;
            continue;
        }
        
        if trimmed.starts_with('[') && trimmed != "[workspace]" {
            if in_workspace && !trimmed.starts_with("[workspace.") {
                in_workspace = false;
                in_members = false;
            }
            continue;
        }
        
        if in_workspace && trimmed.starts_with("members") {
            in_members = true;
            // Check for inline array
            if let Some(start) = trimmed.find('[') {
                if let Some(end) = trimmed.find(']') {
                    // Inline: members = ["foo", "bar"]
                    let array_content = &trimmed[start+1..end];
                    for part in array_content.split(',') {
                        let member_path = part.trim().trim_matches('"');
                        if !member_path.is_empty() {
                            add_workspace_member(&mut members, root, member_path)?;
                        }
                    }
                    in_members = false;
                }
            }
            continue;
        }
        
        if in_members {
            if trimmed == "]" {
                in_members = false;
                continue;
            }
            
            let member_path = trimmed.trim_matches(|c| c == '"' || c == ',' || c == ' ');
            if !member_path.is_empty() && !member_path.starts_with('#') {
                add_workspace_member(&mut members, root, member_path)?;
            }
        }
    }
    
    // Handle glob patterns like "crates/*"
    members = expand_glob_members(members, root);
    
    Ok(members)
}

fn add_workspace_member(
    members: &mut Vec<WorkspaceMember>,
    root: &Path,
    member_path: &str,
) -> Result<(), String> {
    // Skip glob patterns for now (handled separately)
    if member_path.contains('*') {
        // Store as placeholder
        members.push(WorkspaceMember {
            name: member_path.to_string(),
            path: root.join(member_path),
            has_rustsp: false,
        });
        return Ok(());
    }
    
    let full_path = root.join(member_path);
    if !full_path.exists() {
        return Err(format!("Workspace member not found: {}", member_path));
    }
    
    let name = full_path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(member_path)
        .to_string();
    
    let has_rustsp = has_rss_files(&full_path.join("src"));
    
    members.push(WorkspaceMember {
        name,
        path: full_path,
        has_rustsp,
    });
    
    Ok(())
}

fn expand_glob_members(members: Vec<WorkspaceMember>, root: &Path) -> Vec<WorkspaceMember> {
    let mut expanded = Vec::new();
    
    for member in members {
        if member.name.contains('*') {
            // Expand glob
            let pattern_parts: Vec<&str> = member.name.split('*').collect();
            if pattern_parts.len() == 2 {
                let prefix = pattern_parts[0].trim_end_matches('/');
                let base_dir = root.join(prefix);
                
                if let Ok(entries) = fs::read_dir(&base_dir) {
                    for entry in entries.filter_map(|e| e.ok()) {
                        let path = entry.path();
                        if path.is_dir() && path.join("Cargo.toml").exists() {
                            let name = path.file_name()
                                .and_then(|n| n.to_str())
                                .unwrap_or("unknown")
                                .to_string();
                            let has_rustsp = has_rss_files(&path.join("src"));
                            
                            expanded.push(WorkspaceMember {
                                name,
                                path,
                                has_rustsp,
                            });
                        }
                    }
                }
            }
        } else {
            expanded.push(member);
        }
    }
    
    expanded
}

fn has_rss_files(dir: &Path) -> bool {
    if !dir.exists() {
        return false;
    }
    
    fn check_recursive(path: &Path) -> bool {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.filter_map(|e| e.ok()) {
                let p = entry.path();
                if p.is_file() && p.extension().map(|e| e == "rss").unwrap_or(false) {
                    return true;
                }
                if p.is_dir() && p.file_name().map(|n| n != "target").unwrap_or(true) {
                    if check_recursive(&p) {
                        return true;
                    }
                }
            }
        }
        false
    }
    
    check_recursive(dir)
}

//=============================================================================
// Build Configuration
//=============================================================================

#[derive(Debug, Clone)]
struct BuildConfig {
    /// Project root directory
    project_root: PathBuf,
    /// Subcommand: build, run, test, check
    subcommand: String,
    /// Extra arguments passed to cargo
    extra_args: Vec<String>,
    /// Build in release mode
    release: bool,
    /// Quiet mode
    quiet: bool,
    /// Skip cache (force rebuild)
    force_rebuild: bool,
    /// Number of parallel jobs
    jobs: Option<usize>,
    /// Specific package to build (for workspaces)
    package: Option<String>,
    /// Features to enable
    features: Vec<String>,
    /// All features
    all_features: bool,
    /// No default features
    no_default_features: bool,
}

impl BuildConfig {
    fn from_args(args: &[String], project_root: PathBuf) -> Result<Self, String> {
        let mut config = BuildConfig {
            project_root,
            subcommand: String::new(),
            extra_args: Vec::new(),
            release: false,
            quiet: false,
            force_rebuild: false,
            jobs: None,
            package: None,
            features: Vec::new(),
            all_features: false,
            no_default_features: false,
        };
        
        let start_idx = if args.len() > 1 && args[1] == "rustsp" { 2 } else { 1 };
        
        if args.len() <= start_idx {
            return Err("No command specified".to_string());
        }
        
        config.subcommand = args[start_idx].clone();
        
        let mut i = start_idx + 1;
        while i < args.len() {
            let arg = &args[i];
            match arg.as_str() {
                "--release" | "-r" => config.release = true,
                "--quiet" | "-q" => config.quiet = true,
                "--force" | "-f" => config.force_rebuild = true,
                "--all-features" => config.all_features = true,
                "--no-default-features" => config.no_default_features = true,
                "-p" | "--package" => {
                    i += 1;
                    if i < args.len() {
                        config.package = Some(args[i].clone());
                    }
                }
                "-j" | "--jobs" => {
                    i += 1;
                    if i < args.len() {
                        config.jobs = args[i].parse().ok();
                    }
                }
                "--features" | "-F" => {
                    i += 1;
                    if i < args.len() {
                        config.features.extend(
                            args[i].split(',').map(|s| s.trim().to_string())
                        );
                    }
                }
                _ => config.extra_args.push(arg.clone()),
            }
            i += 1;
        }
        
        Ok(config)
    }
}

//=============================================================================
// Path Utilities
//=============================================================================

fn normalize_path(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if s.starts_with(r"\\?\") {
        PathBuf::from(&s[4..])
    } else {
        path.to_path_buf()
    }
}

fn absolute_path(path: &Path) -> Result<PathBuf, String> {
    let abs = fs::canonicalize(path)
        .map_err(|e| format!("Failed to resolve {}: {}", path.display(), e))?;
    Ok(normalize_path(&abs))
}

//=============================================================================
// Cargo.toml Generation
//=============================================================================

fn parse_toml_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with(key) && line.contains('=') {
            if let Some(eq_pos) = line.find('=') {
                let value = line[eq_pos + 1..].trim();
                if value.starts_with('"') && value.ends_with('"') && value.len() >= 2 {
                    return Some(value[1..value.len()-1].to_string());
                }
                return Some(value.to_string());
            }
        }
    }
    None
}

fn extract_dependencies(content: &str) -> String {
    extract_dependencies_with_path_rewrite(content, None)
}

/// Extract dependencies with optional path rewriting for shadow builds.
/// 
/// When `original_crate_root` is provided, all relative path dependencies
/// are rewritten to absolute paths so they work from the shadow directory.
/// 
/// Example:
///   Original (in D:\dsdn\crates\validator):
///     dsdn-common = { path = "../common" }
///   Rewritten (for shadow build):
///     dsdn-common = { path = "D:/dsdn/crates/common" }
fn extract_dependencies_with_path_rewrite(content: &str, original_crate_root: Option<&Path>) -> String {
    let mut result = String::new();
    let mut in_section = false;
    let mut current_section = String::new();
    
    let dep_sections = [
        "[dependencies]",
        "[dev-dependencies]", 
        "[build-dependencies]",
    ];
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        // Check for section headers
        if trimmed.starts_with('[') {
            in_section = false;
            for section in &dep_sections {
                if trimmed == *section || trimmed.starts_with(&format!("{}.", section.trim_end_matches(']'))) {
                    in_section = true;
                    if trimmed != current_section {
                        if !result.is_empty() {
                            result.push('\n');
                        }
                        result.push_str(trimmed);
                        result.push('\n');
                        current_section = trimmed.to_string();
                    }
                    break;
                }
            }
            continue;
        }
        
        if in_section && !trimmed.is_empty() && !trimmed.starts_with('#') {
            // CRITICAL FIX: Rewrite path dependencies to absolute paths
            let processed_line = if let Some(crate_root) = original_crate_root {
                rewrite_path_dependency(line, crate_root)
            } else {
                line.to_string()
            };
            result.push_str(&processed_line);
            result.push('\n');
        }
    }
    
    result
}

/// Normalize a path string for use in Cargo.toml
/// 
/// On Windows, canonicalize() returns paths with \\?\ prefix (extended-length path).
/// This prefix is NOT valid in Cargo.toml, so we must strip it.
/// 
/// Examples:
///   - `\\?\D:\dsdn\crates\common` -> `D:/dsdn/crates/common`
///   - `/home/user/project` -> `/home/user/project` (unchanged)
fn normalize_path_for_cargo(path_str: &str) -> String {
    let mut result = path_str.to_string();
    
    // CRITICAL FIX: Strip Windows extended-length path prefix
    // canonicalize() on Windows returns \\?\C:\... which is invalid for Cargo
    if result.starts_with(r"\\?\") {
        result = result[4..].to_string();
    }
    // Also handle the forward-slash version (in case it was already converted)
    if result.starts_with("//?/") {
        result = result[4..].to_string();
    }
    
    // Convert backslashes to forward slashes (works in Cargo.toml on all platforms)
    result.replace('\\', "/")
}

/// Rewrite a dependency line, converting relative path to absolute.
/// 
/// Handles both inline format and table format:
///   - `dep = { path = "../foo" }` -> `dep = { path = "/abs/path/foo" }`
///   - `path = "../foo"` -> `path = "/abs/path/foo"`
fn rewrite_path_dependency(line: &str, crate_root: &Path) -> String {
    // Look for path = "..." pattern
    if !line.contains("path") {
        return line.to_string();
    }
    
    // Use a more robust approach: find `path = "..."` and replace it
    let mut result = String::new();
    let mut remaining = line;
    
    while let Some(path_pos) = remaining.find("path") {
        // Add everything before "path"
        result.push_str(&remaining[..path_pos]);
        remaining = &remaining[path_pos..];
        
        // Check if this is actually `path = "..."`
        let after_path = &remaining[4..]; // Skip "path"
        let after_path_trimmed = after_path.trim_start();
        
        if !after_path_trimmed.starts_with('=') {
            // Not a path assignment, just the word "path" somewhere
            result.push_str("path");
            remaining = &remaining[4..];
            continue;
        }
        
        // Skip to after the =
        let eq_offset = 4 + (after_path.len() - after_path_trimmed.len());
        let after_eq = &remaining[eq_offset + 1..]; // +1 for '='
        let after_eq_trimmed = after_eq.trim_start();
        
        if !after_eq_trimmed.starts_with('"') {
            // Not a quoted string
            result.push_str("path");
            remaining = &remaining[4..];
            continue;
        }
        
        // Find the content between quotes
        let quote_content_start = eq_offset + 1 + (after_eq.len() - after_eq_trimmed.len()) + 1;
        let content_remaining = &remaining[quote_content_start..];
        
        if let Some(end_quote) = content_remaining.find('"') {
            let rel_path_str = &content_remaining[..end_quote];
            
            // Convert relative path to absolute
            let rel_path = Path::new(rel_path_str);
            if rel_path.is_relative() && (rel_path_str.starts_with("..") || rel_path_str.starts_with("./")) {
                let abs_path = crate_root.join(rel_path);
                
                // CRITICAL FIX: Use normalize_path_for_cargo to handle Windows \\?\ prefix
                let abs_str = if let Ok(canonical) = abs_path.canonicalize() {
                    normalize_path_for_cargo(&canonical.to_string_lossy())
                } else {
                    normalize_path_for_cargo(&abs_path.to_string_lossy())
                };
                
                // Build the replacement
                result.push_str("path = \"");
                result.push_str(&abs_str);
                result.push('"');
                
                // Continue after the closing quote
                remaining = &content_remaining[end_quote + 1..];
            } else {
                // Not a relative path we need to rewrite, keep as-is
                result.push_str(&remaining[..quote_content_start + end_quote + 1]);
                remaining = &content_remaining[end_quote + 1..];
            }
        } else {
            // No closing quote found, keep as-is
            result.push_str("path");
            remaining = &remaining[4..];
        }
    }
    
    // Add any remaining content
    result.push_str(remaining);
    result
}

fn extract_features_section(content: &str) -> String {
    let mut result = String::new();
    let mut in_features = false;
    let mut brace_depth = 0;
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        if trimmed == "[features]" {
            in_features = true;
            result.push_str("[features]\n");
            continue;
        }
        
        if in_features {
            if trimmed.starts_with('[') && trimmed != "[features]" {
                break;
            }
            if !trimmed.is_empty() {
                result.push_str(line);
                result.push('\n');
            }
        }
    }
    
    result
}

fn generate_cargo_toml(
    original_toml: &str,
    has_main: bool,
    has_lib: bool,
    project_name: &str,
    crate_root: Option<&Path>,
) -> String {
    let name = parse_toml_value(original_toml, "name")
        .unwrap_or_else(|| project_name.to_string());
    let version = parse_toml_value(original_toml, "version")
        .unwrap_or_else(|| "0.1.0".to_string());
    let edition = parse_toml_value(original_toml, "edition")
        .unwrap_or_else(|| "2021".to_string());
    
    let mut toml = String::new();
    
    // Package section
    toml.push_str("[package]\n");
    toml.push_str(&format!("name = \"{}\"\n", name));
    toml.push_str(&format!("version = \"{}\"\n", version));
    toml.push_str(&format!("edition = \"{}\"\n", edition));
    
    // Copy other package fields
    // CRITICAL FIX: Some fields are arrays (authors, keywords, categories)
    // and should NOT be wrapped in extra quotes!
    for field in ["authors", "description", "license", "repository", "readme", "keywords", "categories"] {
        if let Some(value) = parse_toml_value(original_toml, field) {
            // CRITICAL FIX: Check if value is an array or already has quotes
            // Arrays start with '[', don't wrap them in extra quotes!
            if value.starts_with('[') {
                // Array value - use as-is (e.g., authors = ["INEVA team"])
                toml.push_str(&format!("{} = {}\n", field, value));
            } else {
                // String value - wrap in quotes
                toml.push_str(&format!("{} = \"{}\"\n", field, value));
            }
        }
    }
    toml.push('\n');
    
    // Binary target
    if has_main {
        toml.push_str("[[bin]]\n");
        toml.push_str(&format!("name = \"{}\"\n", name));
        toml.push_str("path = \"src/main.rs\"\n\n");
    }
    
    // Library target
    // CRITICAL FIX: When package name has dashes (e.g., "dsdn-validator"),
    // the library crate name must use underscores (e.g., "dsdn_validator")
    // so that `use dsdn_validator::*` works from the binary.
    if has_lib {
        let lib_name = name.replace('-', "_");
        toml.push_str("[lib]\n");
        toml.push_str(&format!("name = \"{}\"\n", lib_name));
        toml.push_str("path = \"src/lib.rs\"\n\n");
    }
    
    // Dependencies - CRITICAL FIX: Rewrite path dependencies to absolute paths
    let deps = extract_dependencies_with_path_rewrite(original_toml, crate_root);
    if !deps.is_empty() {
        toml.push_str(&deps);
        toml.push('\n');
    }
    
    // Features
    let features = extract_features_section(original_toml);
    if !features.is_empty() {
        toml.push_str(&features);
    }
    
    toml
}

//=============================================================================
// RustS+ Compilation
//=============================================================================

fn find_rustsp_binary() -> String {
    // Check common names in PATH
    for cmd in &["rustsp", "rusts_plus", "rustsp.exe", "rusts_plus.exe"] {
        if Command::new(cmd)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
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

fn compile_rss_file(
    rustsp_cmd: &str,
    input: &Path,
    output: &Path,
    quiet: bool,
) -> Result<bool, String> {
    // Ensure output directory exists
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create {}: {}", parent.display(), e))?;
    }
    
    let mut cmd = Command::new(rustsp_cmd);
    cmd.arg(input)
        .arg("--emit-rs")
        .arg("-o")
        .arg(output);
    
    if quiet {
        cmd.arg("--quiet");
    }
    
    let result = cmd.output()
        .map_err(|e| format!("Failed to run {}: {}", rustsp_cmd, e))?;
    
    if !result.status.success() {
        // Print errors with source context
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        
        if !stderr.is_empty() {
            eprintln!("{}", stderr);
        }
        if !stdout.is_empty() {
            eprintln!("{}", stdout);
        }
        
        return Ok(false);
    }
    
    if !output.exists() {
        return Err(format!("Compiler did not create output: {}", output.display()));
    }
    
    Ok(true)
}

//=============================================================================
// Build Pipeline
//=============================================================================

fn get_shadow_dir(project_name: &str) -> PathBuf {
    let temp_base = env::temp_dir();
    temp_base.join(format!("rustsp_shadow_{}", project_name))
}

struct BuildContext {
    config: BuildConfig,
    cache: FileCache,
    shadow_dir: PathBuf,
    target_dir: PathBuf,
    rustsp_cmd: String,
}

impl BuildContext {
    fn new(config: BuildConfig) -> Result<Self, String> {
        let project_name = config.project_root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rustsp_project")
            .to_string();
        
        let shadow_dir = get_shadow_dir(&project_name);
        let target_dir = config.project_root.join("target").join("rustsp_build");
        let cache = FileCache::new(&target_dir);
        let rustsp_cmd = find_rustsp_binary();
        
        Ok(BuildContext {
            config,
            cache,
            shadow_dir,
            target_dir,
            rustsp_cmd,
        })
    }
    
    fn log(&self, prefix: &str, msg: &str) {
        if !self.config.quiet {
            eprintln!("{}{:>12}{} {}", ansi::BOLD_CYAN, prefix, ansi::RESET, msg);
        }
    }
    
    fn log_dim(&self, msg: &str) {
        if !self.config.quiet {
            eprintln!("  {}{}{}", ansi::DIM, msg, ansi::RESET);
        }
    }
}

fn build_single_crate(ctx: &mut BuildContext, crate_root: &Path) -> Result<(), String> {
    let crate_root = absolute_path(crate_root)?;
    let src_dir = crate_root.join("src");
    
    if !src_dir.exists() {
        return Err(format!("No src/ directory found in {}", crate_root.display()));
    }
    
    // Build module graph
    ctx.log("Analyzing", &format!("{}", crate_root.display()));
    let shadow_src = ctx.shadow_dir.join("src");
    let graph = build_module_graph(&src_dir, &shadow_src)?;
    
    if graph.is_empty() {
        return Err("No source files found".to_string());
    }
    
    // Check if any .rss files exist
    if graph.rss_count() == 0 {
        ctx.log("Skipping", "No .rss files found, running plain cargo");
        return run_plain_cargo(&crate_root, &ctx.config);
    }
    
    // Clean and recreate shadow directory
    if ctx.shadow_dir.exists() {
        fs::remove_dir_all(&ctx.shadow_dir)
            .map_err(|e| format!("Failed to clean {}: {}", ctx.shadow_dir.display(), e))?;
    }
    fs::create_dir_all(&shadow_src)
        .map_err(|e| format!("Failed to create {}: {}", shadow_src.display(), e))?;
    
    // Compile/copy files
    ctx.log("Compiling", &format!("{} .rss + {} .rs files", graph.rss_count(), graph.rs_count()));
    
    let mut any_error = false;
    let mut compiled_count = 0;
    let mut cached_count = 0;
    
    for (rel_path, node) in &graph.nodes {
        // Ensure output directory exists
        if let Some(parent) = node.output_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        
        if node.is_rustsp {
            // Check cache
            let content = fs::read_to_string(&node.source_path)
                .map_err(|e| format!("Failed to read {}: {}", node.source_path.display(), e))?;
            let content_hash = hash_content(&content);
            
            let needs_compile = ctx.config.force_rebuild || 
                                ctx.cache.needs_rebuild(&node.source_path, content_hash);
            
            if needs_compile {
                let rel_display = node.source_path.strip_prefix(&src_dir)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| rel_path.clone());
                ctx.log_dim(&format!("{} -> {}", rel_display, 
                    node.output_path.file_name().unwrap().to_string_lossy()));
                
                if !compile_rss_file(&ctx.rustsp_cmd, &node.source_path, &node.output_path, ctx.config.quiet)? {
                    any_error = true;
                } else {
                    ctx.cache.update(node.source_path.clone(), content_hash, node.output_path.clone());
                    compiled_count += 1;
                }
            } else {
                // Use cached version - still need to copy to shadow
                if let Some(cached) = ctx.cache.entries.get(&node.source_path) {
                    if cached.output_path.exists() {
                        fs::copy(&cached.output_path, &node.output_path).ok();
                    }
                }
                cached_count += 1;
            }
        } else {
            // Copy .rs file as-is
            fs::copy(&node.source_path, &node.output_path)
                .map_err(|e| format!("Failed to copy {}: {}", node.source_path.display(), e))?;
        }
    }
    
    if any_error {
        return Err("RustS+ compilation failed".to_string());
    }
    
    if compiled_count > 0 || cached_count > 0 {
        ctx.log("Preprocessed", &format!("{} compiled, {} cached", compiled_count, cached_count));
    }
    
    // Save cache
    ctx.cache.save().ok();
    
    // Check entry points
    let has_main = shadow_src.join("main.rs").exists();
    let has_lib = shadow_src.join("lib.rs").exists();
    
    if !has_main && !has_lib {
        return Err("No main.rs or lib.rs generated".to_string());
    }
    
    // Generate Cargo.toml
    let original_toml = fs::read_to_string(crate_root.join("Cargo.toml"))
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    
    let project_name = crate_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rustsp_project");
    
    // CRITICAL FIX: Pass crate_root so path dependencies can be rewritten to absolute paths
    let new_toml = generate_cargo_toml(&original_toml, has_main, has_lib, project_name, Some(&crate_root));
    fs::write(ctx.shadow_dir.join("Cargo.toml"), &new_toml)
        .map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;
    
    // Run cargo
    ctx.log("Building", &format!("cargo {}", ctx.config.subcommand));
    run_cargo_in_shadow(ctx)
}

fn run_plain_cargo(crate_root: &Path, config: &BuildConfig) -> Result<(), String> {
    // Safety check: verify that at least one target exists
    let src_dir = crate_root.join("src");
    let has_main_rs = src_dir.join("main.rs").exists();
    let has_lib_rs = src_dir.join("lib.rs").exists();
    
    if !has_main_rs && !has_lib_rs {
        return Err(format!(
            "Cannot run plain cargo: no src/main.rs or src/lib.rs found.\n\
             If you have .rss files, make sure you have a main.rss or lib.rss entry point."
        ));
    }
    
    let mut cmd = Command::new("cargo");
    cmd.current_dir(crate_root)
        .arg(&config.subcommand);
    
    if config.release {
        cmd.arg("--release");
    }
    
    cmd.args(&config.extra_args);
    
    let status = cmd.status()
        .map_err(|e| format!("Failed to run cargo: {}", e))?;
    
    if !status.success() {
        return Err("Cargo failed".to_string());
    }
    Ok(())
}

fn run_cargo_in_shadow(ctx: &BuildContext) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(&ctx.shadow_dir)
        .arg(&ctx.config.subcommand)
        .arg("--target-dir")
        .arg(&ctx.target_dir);
    
    if ctx.config.release {
        cmd.arg("--release");
    }
    
    if let Some(jobs) = ctx.config.jobs {
        cmd.arg("-j").arg(jobs.to_string());
    }
    
    if ctx.config.all_features {
        cmd.arg("--all-features");
    }
    
    if ctx.config.no_default_features {
        cmd.arg("--no-default-features");
    }
    
    if !ctx.config.features.is_empty() {
        cmd.arg("--features").arg(ctx.config.features.join(","));
    }
    
    cmd.args(&ctx.config.extra_args);
    
    let status = cmd.status()
        .map_err(|e| format!("Failed to run cargo: {}", e))?;
    
    if !status.success() {
        return Err("Cargo build failed".to_string());
    }
    
    // Report output location
    let profile = if ctx.config.release { "release" } else { "debug" };
    let output_dir = ctx.target_dir.join(profile);
    
    if !ctx.config.quiet {
        eprintln!("{}    Finished{} {} [{}]", 
            ansi::BOLD_GREEN, ansi::RESET,
            ctx.config.subcommand,
            output_dir.display());
    }
    
    Ok(())
}

//=============================================================================
// Workspace Build
//=============================================================================

fn build_workspace(ctx: &mut BuildContext, members: &[WorkspaceMember]) -> Result<(), String> {
    ctx.log("Workspace", &format!("{} members", members.len()));
    
    // Filter to members with .rss files (or all if --package specified)
    let to_build: Vec<&WorkspaceMember> = if let Some(ref pkg) = ctx.config.package {
        members.iter().filter(|m| m.name == *pkg).collect()
    } else {
        members.iter().filter(|m| m.has_rustsp).collect()
    };
    
    if to_build.is_empty() {
        if ctx.config.package.is_some() {
            return Err(format!("Package '{}' not found", ctx.config.package.as_ref().unwrap()));
        }
        ctx.log("Skipping", "No workspace members have .rss files");
        return Ok(());
    }
    
    // Build each member
    let mut failed = Vec::new();
    
    for member in to_build {
        ctx.log("Building", &format!("member: {}", member.name));
        
        // Create member-specific shadow dir
        let member_shadow = ctx.shadow_dir.join(&member.name);
        let original_shadow = ctx.shadow_dir.clone();
        ctx.shadow_dir = member_shadow;
        
        match build_single_crate(ctx, &member.path) {
            Ok(()) => {}
            Err(e) => {
                eprintln!("{}error{}: {} failed: {}", ansi::BOLD_RED, ansi::RESET, member.name, e);
                failed.push(member.name.clone());
            }
        }
        
        ctx.shadow_dir = original_shadow;
    }
    
    if !failed.is_empty() {
        return Err(format!("Failed to build: {}", failed.join(", ")));
    }
    
    Ok(())
}

//=============================================================================
// Clean Command
//=============================================================================

fn clean_rustsp_artifacts(project_root: &Path, quiet: bool) {
    let artifacts = [
        project_root.join("target/rustsp_build"),
    ];
    
    for path in &artifacts {
        if path.exists() {
            if !quiet {
                eprintln!("{}   Cleaning{} {}", ansi::BOLD_CYAN, ansi::RESET, path.display());
            }
            let _ = fs::remove_dir_all(path);
        }
    }
    
    // Clean shadow directory
    if let Some(name) = project_root.file_name().and_then(|n| n.to_str()) {
        let shadow = get_shadow_dir(name);
        if shadow.exists() {
            if !quiet {
                eprintln!("{}   Cleaning{} {}", ansi::BOLD_CYAN, ansi::RESET, shadow.display());
            }
            let _ = fs::remove_dir_all(&shadow);
        }
    }
}

//=============================================================================
// Main Entry Point
//=============================================================================

fn print_usage() {
    eprintln!("{}cargo-rustsp{} v0.9.0 - Robust Cargo Integration for RustS+", 
        ansi::BOLD_CYAN, ansi::RESET);
    eprintln!();
    eprintln!("{}USAGE:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    cargo rustsp <COMMAND> [OPTIONS]");
    eprintln!();
    eprintln!("{}COMMANDS:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    build      Compile the current package");
    eprintln!("    run        Run the main binary");
    eprintln!("    test       Run tests");
    eprintln!("    check      Check for errors without building");
    eprintln!("    clean      Remove build artifacts");
    eprintln!();
    eprintln!("{}OPTIONS:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    -r, --release           Build in release mode");
    eprintln!("    -q, --quiet             Suppress output");
    eprintln!("    -f, --force             Force rebuild (ignore cache)");
    eprintln!("    -p, --package <SPEC>    Build specific package");
    eprintln!("    -j, --jobs <N>          Number of parallel jobs");
    eprintln!("    -F, --features <F>      Features to activate");
    eprintln!("    --all-features          Activate all features");
    eprintln!("    --no-default-features   Disable default features");
    eprintln!();
    eprintln!("{}EXAMPLES:{}", ansi::BOLD, ansi::RESET);
    eprintln!("    cargo rustsp build");
    eprintln!("    cargo rustsp run --release");
    eprintln!("    cargo rustsp test -p my-crate");
    eprintln!("    cargo rustsp build --features=async");
}

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

fn main() {
    let args: Vec<String> = env::args().collect();
    let start_idx = if args.len() > 1 && args[1] == "rustsp" { 2 } else { 1 };
    
    // Handle help/version
    if args.len() <= start_idx {
        print_usage();
        exit(1);
    }
    
    let action = &args[start_idx];
    
    if action == "-h" || action == "--help" || action == "help" {
        print_usage();
        exit(0);
    }
    
    if action == "-V" || action == "--version" {
        println!("cargo-rustsp 0.9.0");
        exit(0);
    }
    
    // Validate command
    let valid_commands = ["build", "run", "test", "check", "clean", "bench", "doc"];
    if !valid_commands.contains(&action.as_str()) {
        eprintln!("{}error{}: unsupported command '{}'", ansi::BOLD_RED, ansi::RESET, action);
        eprintln!("\nRun 'cargo rustsp --help' for usage");
        exit(1);
    }
    
    // Find project root
    let project_root = match find_project_root() {
        Some(root) => root,
        None => {
            eprintln!("{}error{}: could not find Cargo.toml in {} or any parent directory",
                ansi::BOLD_RED, ansi::RESET,
                env::current_dir().map(|p| p.display().to_string()).unwrap_or_default());
            exit(1);
        }
    };
    
    // Handle clean specially
    if action == "clean" {
        let quiet = args.iter().any(|a| a == "-q" || a == "--quiet");
        clean_rustsp_artifacts(&project_root, quiet);
        
        // Also clean standard target/ if it exists (for mixed projects)
        // We do this manually instead of `cargo clean` because:
        // - Original project may only have .rss files, no .rs files
        // - Cargo would fail with "no targets specified" error
        let standard_target = project_root.join("target");
        if standard_target.exists() {
            // Only clean if it's NOT the rustsp_build directory itself
            let rustsp_build = standard_target.join("rustsp_build");
            if !rustsp_build.exists() || standard_target.read_dir().map(|mut d| d.next().is_some()).unwrap_or(false) {
                if !quiet {
                    eprintln!("{}   Cleaning{} {}", ansi::BOLD_CYAN, ansi::RESET, standard_target.display());
                }
                let _ = fs::remove_dir_all(&standard_target);
            }
        }
        
        if !quiet {
            eprintln!("{}   Cleaned{} RustS+ build artifacts", ansi::BOLD_GREEN, ansi::RESET);
        }
        
        exit(0);
    }
    
    // Parse config
    let config = match BuildConfig::from_args(&args, project_root.clone()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    };
    
    // Create build context
    let mut ctx = match BuildContext::new(config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    };
    
    // Detect project kind
    let project_kind = match detect_project_kind(&project_root) {
        Ok(k) => k,
        Err(e) => {
            eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
            exit(1);
        }
    };
    
    // Run build
    let result = match project_kind {
        ProjectKind::SingleCrate => build_single_crate(&mut ctx, &project_root),
        ProjectKind::Workspace(members) => build_workspace(&mut ctx, &members),
    };
    
    if let Err(e) = result {
        eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
        exit(1);
    }
}

//=============================================================================
// Tests
//=============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_mod_extraction() {
        let source = r#"
            mod foo;
            pub mod bar;
            // mod commented;
            mod baz;
        "#;
        
        let mods = extract_mod_declarations(source);
        assert_eq!(mods.len(), 3);
        assert_eq!(mods[0].0, "foo");
        assert_eq!(mods[1].0, "bar");
        assert_eq!(mods[2].0, "baz");
    }
    
    #[test]
    fn test_valid_module_name() {
        assert!(is_valid_module_name("foo"));
        assert!(is_valid_module_name("foo_bar"));
        assert!(is_valid_module_name("_private"));
        assert!(!is_valid_module_name("Foo"));
        assert!(!is_valid_module_name("123"));
        assert!(!is_valid_module_name(""));
    }
    
    #[test]
    fn test_cargo_toml_generation() {
        let original = r#"
[package]
name = "test-project"
version = "1.0.0"
edition = "2021"

[dependencies]
serde = "1.0"
"#;
        
        let generated = generate_cargo_toml(original, true, true, "test", None);
        assert!(generated.contains("name = \"test-project\""));
        assert!(generated.contains("[[bin]]"));
        assert!(generated.contains("[lib]"));
        assert!(generated.contains("[dependencies]"));
        assert!(generated.contains("serde"));
    }
    
    #[test]
    fn test_cargo_toml_with_authors_array() {
        // CRITICAL TEST: authors is an ARRAY, not a string!
        // Must not be wrapped in extra quotes
        let original = r#"
[package]
name = "dsdn-validator"
version = "0.1.0"
edition = "2021"
authors = ["INEVA team"]
keywords = ["blockchain", "storage"]

[dependencies]
serde = "1.0"
"#;
        
        let generated = generate_cargo_toml(original, false, true, "validator", None);
        
        // authors should be an array, NOT wrapped in quotes
        assert!(generated.contains("authors = [\"INEVA team\"]"), 
            "authors should be array, got: {}", generated);
        // keywords should also be an array
        assert!(generated.contains("keywords = [\"blockchain\", \"storage\"]"),
            "keywords should be array, got: {}", generated);
        // Should NOT contain the buggy double-quoted array
        assert!(!generated.contains("authors = \"["), 
            "authors should NOT be double-quoted, got: {}", generated);
        
        // CRITICAL: Library name should have underscores, not dashes
        // so `use dsdn_validator::*` works
        assert!(generated.contains("name = \"dsdn_validator\""),
            "Library name should use underscores, got: {}", generated);
    }
    
    #[test]
    fn test_path_dependency_rewrite() {
        use std::path::PathBuf;
        
        // Test rewriting path dependencies
        let line = r#"dsdn-common = { path = "../common", version = "0.1" }"#;
        let crate_root = PathBuf::from("/project/crates/validator");
        
        let result = rewrite_path_dependency(line, &crate_root);
        
        // Should contain absolute path
        assert!(result.contains("/project/crates/common") || result.contains("\\project\\crates\\common"),
            "Path should be absolute, got: {}", result);
        // Should still have version
        assert!(result.contains("version = \"0.1\""),
            "Should preserve other fields, got: {}", result);
    }
    
    #[test]
    fn test_normalize_path_for_cargo() {
        // Test Windows extended-length path prefix stripping
        assert_eq!(
            normalize_path_for_cargo(r"\\?\D:\dsdn\crates\common"),
            "D:/dsdn/crates/common"
        );
        
        // Test forward-slash version (in case already converted incorrectly)
        assert_eq!(
            normalize_path_for_cargo("//?/D:/dsdn/crates/common"),
            "D:/dsdn/crates/common"
        );
        
        // Test normal paths are unchanged (except backslash conversion)
        assert_eq!(
            normalize_path_for_cargo(r"D:\dsdn\crates\common"),
            "D:/dsdn/crates/common"
        );
        
        // Test Unix paths are unchanged
        assert_eq!(
            normalize_path_for_cargo("/home/user/project"),
            "/home/user/project"
        );
    }
}