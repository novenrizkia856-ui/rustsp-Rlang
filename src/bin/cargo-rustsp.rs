//! cargo-rustsp - Cargo Integration for RustS+
//! v0.8.0 - Use TEMP directory to completely isolate from parent Cargo.toml

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, exit};
use std::io::{self, Write};
use std::time::{SystemTime, UNIX_EPOCH};

mod ansi {
    pub const RESET: &str = "\x1b[0m";
    pub const BOLD_RED: &str = "\x1b[1;31m";
    pub const BOLD_GREEN: &str = "\x1b[1;32m";
    pub const BOLD_CYAN: &str = "\x1b[1;36m";
    pub const CYAN: &str = "\x1b[36m";
    pub const DIM: &str = "\x1b[2m";
}

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

fn parse_toml_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.starts_with(key) {
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
    let mut in_deps = false;
    let mut deps = String::new();
    
    for line in content.lines() {
        let trimmed = line.trim();
        
        if trimmed.starts_with('[') {
            if trimmed == "[dependencies]" {
                in_deps = true;
                deps.push_str("[dependencies]\n");
                continue;
            } else if trimmed == "[dev-dependencies]" {
                in_deps = true;
                deps.push_str("\n[dev-dependencies]\n");
                continue;
            } else if trimmed == "[build-dependencies]" {
                in_deps = true;
                deps.push_str("\n[build-dependencies]\n");
                continue;
            } else {
                in_deps = false;
                continue;
            }
        }
        
        if in_deps && !trimmed.is_empty() && !trimmed.starts_with('#') {
            deps.push_str(line);
            deps.push('\n');
        }
    }
    
    deps
}

fn generate_cargo_toml(
    original_toml: &str,
    has_main: bool,
    has_lib: bool,
    project_name: &str,
) -> String {
    let name = parse_toml_value(original_toml, "name")
        .unwrap_or_else(|| project_name.to_string());
    let version = parse_toml_value(original_toml, "version")
        .unwrap_or_else(|| "0.1.0".to_string());
    let edition = parse_toml_value(original_toml, "edition")
        .unwrap_or_else(|| "2021".to_string());
    
    let mut toml = String::new();
    
    toml.push_str("[package]\n");
    toml.push_str(&format!("name = \"{}\"\n", name));
    toml.push_str(&format!("version = \"{}\"\n", version));
    toml.push_str(&format!("edition = \"{}\"\n", edition));
    toml.push('\n');
    
    if has_main {
        toml.push_str("[[bin]]\n");
        toml.push_str(&format!("name = \"{}\"\n", name));
        toml.push_str("path = \"src/main.rs\"\n");
        toml.push('\n');
    }
    
    if has_lib {
        toml.push_str("[lib]\n");
        toml.push_str("path = \"src/lib.rs\"\n");
        toml.push('\n');
    }
    
    let deps = extract_dependencies(original_toml);
    if !deps.is_empty() {
        toml.push_str(&deps);
    }
    
    toml
}

/// Get a unique temp directory for the shadow project
fn get_shadow_dir(project_name: &str) -> PathBuf {
    let temp_base = env::temp_dir();
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    
    // Use a consistent name so rebuilds use the same dir (for caching)
    // But also include a way to identify it
    temp_base.join(format!("rustsp_shadow_{}", project_name))
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let start_idx = if args.len() > 1 && args[1] == "rustsp" { 2 } else { 1 };
    
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
        println!("cargo-rustsp 0.8.0");
        exit(0);
    }
    
    let valid_commands = ["build", "run", "test", "check", "clean"];
    if !valid_commands.contains(&action.as_str()) {
        eprintln!("{}error{}: unsupported command '{}'", ansi::BOLD_RED, ansi::RESET, action);
        exit(1);
    }
    
    if action == "clean" {
        clean_rustsp_artifacts();
        let status = Command::new("cargo").arg("clean").status().expect("Failed");
        exit(if status.success() { 0 } else { 1 });
    }
    
    let extra_args: Vec<String> = args[start_idx + 1..].iter().cloned().collect();
    let subcommand = action.clone();
    
    let project_root = find_project_root().unwrap_or_else(|| {
        eprintln!("{}error{}: Could not find Cargo.toml", ansi::BOLD_RED, ansi::RESET);
        exit(1);
    });
    
    if let Err(e) = run_rustsp_pipeline(&project_root, &subcommand, &extra_args) {
        eprintln!("{}error{}: {}", ansi::BOLD_RED, ansi::RESET, e);
        exit(1);
    }
}

fn print_usage() {
    eprintln!("{}cargo-rustsp{} - Cargo Integration for RustS+", ansi::BOLD_CYAN, ansi::RESET);
    eprintln!("\nUSAGE: cargo rustsp <COMMAND> [OPTIONS]");
    eprintln!("\nCOMMANDS: build, run, test, check, clean");
    eprintln!("OPTIONS:  --release, -p <SPEC>");
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

fn clean_rustsp_artifacts() {
    // Clean local artifacts
    for dir in &["target/rustsp", "target/rustsp_build"] {
        let path = Path::new(dir);
        if path.exists() {
            eprintln!("{}Cleaning{} {}", ansi::BOLD_CYAN, ansi::RESET, dir);
            let _ = fs::remove_dir_all(path);
        }
    }
    
    // Also try to clean temp shadow directories
    if let Some(root) = find_project_root() {
        if let Some(name) = root.file_name().and_then(|n| n.to_str()) {
            let shadow = get_shadow_dir(name);
            if shadow.exists() {
                eprintln!("{}Cleaning{} {}", ansi::BOLD_CYAN, ansi::RESET, shadow.display());
                let _ = fs::remove_dir_all(&shadow);
            }
        }
    }
}

fn run_rustsp_pipeline(project_root: &Path, subcommand: &str, extra_args: &[String]) -> Result<(), String> {
    let project_root = absolute_path(project_root)?;
    
    let project_name = project_root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rustsp_project")
        .to_string();
    
    // CRITICAL: Use TEMP directory, NOT a subdirectory of the project
    // This completely isolates from parent Cargo.toml
    let shadow_dir = get_shadow_dir(&project_name);
    let shadow_src = shadow_dir.join("src");
    
    // Clean and recreate shadow directory
    if shadow_dir.exists() {
        fs::remove_dir_all(&shadow_dir)
            .map_err(|e| format!("Failed to clean {}: {}", shadow_dir.display(), e))?;
    }
    fs::create_dir_all(&shadow_src)
        .map_err(|e| format!("Failed to create {}: {}", shadow_src.display(), e))?;
    
    let src_dir = project_root.join("src");
    if !src_dir.exists() {
        return Err("No src/ directory found".to_string());
    }
    
    eprintln!("{}Preprocessing{} RustS+ files...", ansi::BOLD_CYAN, ansi::RESET);
    
    let mut rss_files: Vec<PathBuf> = Vec::new();
    let mut rs_files: Vec<PathBuf> = Vec::new();
    collect_source_files(&src_dir, &mut rss_files, &mut rs_files)?;
    
    if rss_files.is_empty() && rs_files.is_empty() {
        return Err("No source files found in src/".to_string());
    }
    
    if rss_files.is_empty() {
        eprintln!("{}note{}: No .rss files, running plain cargo...", ansi::CYAN, ansi::RESET);
        return run_plain_cargo(&project_root, subcommand, extra_args);
    }
    
    // Compile .rss files
    let mut any_error = false;
    for rss_path in &rss_files {
        let rel_path = rss_path.strip_prefix(&src_dir).unwrap();
        let mut rs_rel_path = rel_path.to_path_buf();
        rs_rel_path.set_extension("rs");
        
        let output_path = shadow_src.join(&rs_rel_path);
        
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        
        eprintln!("  {}{} -> {}{}", ansi::DIM, rel_path.display(), rs_rel_path.display(), ansi::RESET);
        
        if !compile_rss_to_rs(rss_path, &output_path)? {
            any_error = true;
        }
    }
    
    if any_error {
        return Err("RustS+ preprocessing failed".to_string());
    }
    
    // Copy .rs files
    for rs_path in &rs_files {
        let rel_path = rs_path.strip_prefix(&src_dir).unwrap();
        let output_path = shadow_src.join(rel_path);
        
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).ok();
        }
        fs::copy(rs_path, &output_path)
            .map_err(|e| format!("Failed to copy {}: {}", rs_path.display(), e))?;
    }
    
    // Check what entry points we have
    let has_main = shadow_src.join("main.rs").exists();
    let has_lib = shadow_src.join("lib.rs").exists();
    
    if !has_main && !has_lib {
        let files: Vec<_> = fs::read_dir(&shadow_src)
            .map(|rd| rd.filter_map(|e| e.ok()).map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        return Err(format!("No main.rs or lib.rs generated. Files: {:?}", files));
    }
    
    // Read original Cargo.toml for metadata
    let original_toml = fs::read_to_string(project_root.join("Cargo.toml"))
        .map_err(|e| format!("Failed to read Cargo.toml: {}", e))?;
    
    // Generate new Cargo.toml
    let new_toml = generate_cargo_toml(&original_toml, has_main, has_lib, &project_name);
    
    eprintln!("  {}Generating Cargo.toml...{}", ansi::DIM, ansi::RESET);
    
    fs::write(shadow_dir.join("Cargo.toml"), &new_toml)
        .map_err(|e| format!("Failed to write Cargo.toml: {}", e))?;
    
    eprintln!("{}Running{} cargo {}...", ansi::BOLD_CYAN, ansi::RESET, subcommand);
    
    // Target dir stays in original project for convenience
    let target_dir = project_root.join("target").join("rustsp_build");
    
    run_cargo_in_shadow(&shadow_dir, &target_dir, subcommand, extra_args)
}

fn collect_source_files(dir: &Path, rss_files: &mut Vec<PathBuf>, rs_files: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = fs::read_dir(dir).map_err(|e| format!("Failed to read {}: {}", dir.display(), e))?;
    
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        
        if path.is_dir() {
            if path.file_name().map(|n| n == "target").unwrap_or(false) {
                continue;
            }
            collect_source_files(&path, rss_files, rs_files)?;
        } else if path.is_file() {
            match path.extension().and_then(|e| e.to_str()) {
                Some("rss") => rss_files.push(path),
                Some("rs") => rs_files.push(path),
                _ => {}
            }
        }
    }
    Ok(())
}

fn compile_rss_to_rs(input: &Path, output: &Path) -> Result<bool, String> {
    let rustsp_cmd = find_rustsp_binary();
    
    let result = Command::new(&rustsp_cmd)
        .arg(input)
        .arg("--emit-rs")
        .arg("-o")
        .arg(output)
        .arg("--quiet")
        .output()
        .map_err(|e| format!("Failed to run {}: {}", rustsp_cmd, e))?;
    
    if !result.status.success() {
        io::stderr().write_all(&result.stderr).ok();
        io::stderr().write_all(&result.stdout).ok();
        return Ok(false);
    }
    
    if !output.exists() {
        return Err(format!("rustsp did not create: {}", output.display()));
    }
    
    Ok(true)
}

fn find_rustsp_binary() -> String {
    for cmd in &["rustsp", "rusts_plus", "rustsp.exe", "rusts_plus.exe"] {
        if Command::new(cmd).arg("--version").output().is_ok() {
            return cmd.to_string();
        }
    }
    
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

fn run_plain_cargo(project_root: &Path, subcommand: &str, extra_args: &[String]) -> Result<(), String> {
    let status = Command::new("cargo")
        .current_dir(project_root)
        .arg(subcommand)
        .args(extra_args)
        .status()
        .map_err(|e| format!("Failed to run cargo: {}", e))?;
    
    if !status.success() {
        return Err("Cargo failed".to_string());
    }
    Ok(())
}

fn run_cargo_in_shadow(shadow_dir: &Path, target_dir: &Path, subcommand: &str, extra_args: &[String]) -> Result<(), String> {
    let shadow_dir = normalize_path(shadow_dir);
    
    eprintln!("  {}Shadow: {}{}", ansi::DIM, shadow_dir.display(), ansi::RESET);
    eprintln!("  {}Target: {}{}", ansi::DIM, target_dir.display(), ansi::RESET);
    
    // Verify Cargo.toml exists
    let manifest = shadow_dir.join("Cargo.toml");
    if !manifest.exists() {
        return Err(format!("Cargo.toml not found: {}", manifest.display()));
    }
    
    // Run cargo from the shadow directory
    // Since shadow_dir is in TEMP (not under project), there's NO parent Cargo.toml to find!
    let status = Command::new("cargo")
        .current_dir(&shadow_dir)
        .arg(subcommand)
        .arg("--target-dir")
        .arg(target_dir)
        .args(extra_args)
        .status()
        .map_err(|e| format!("Failed to run cargo: {}", e))?;
    
    if !status.success() {
        return Err("Cargo build failed".to_string());
    }
    
    eprintln!("{}Build artifacts:{} {}", ansi::BOLD_GREEN, ansi::RESET, target_dir.display());
    Ok(())
}