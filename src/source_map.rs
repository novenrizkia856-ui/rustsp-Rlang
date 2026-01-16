//! Source Map - Maps generated Rust code back to original RustS+ source
//!
//! This module enables accurate error reporting by tracking the correspondence
//! between generated `.rs` lines and original `.rss` lines.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::fs;

/// A mapping from generated line numbers to original source locations
#[derive(Debug, Clone, Default)]
pub struct SourceMap {
    /// Original source file path
    pub source_file: PathBuf,
    /// Map: generated_line -> original_line
    pub line_map: HashMap<usize, usize>,
    /// Map: generated_line -> original_column (if different)
    pub column_map: HashMap<usize, usize>,
    /// Original source content for context display
    pub original_content: String,
}

impl SourceMap {
    pub fn new(source_file: PathBuf) -> Self {
        let original_content = fs::read_to_string(&source_file)
            .unwrap_or_default();
        
        SourceMap {
            source_file,
            line_map: HashMap::new(),
            column_map: HashMap::new(),
            original_content,
        }
    }
    
    /// Record a line mapping
    pub fn map_line(&mut self, generated: usize, original: usize) {
        self.line_map.insert(generated, original);
    }
    
    /// Record a column mapping
    pub fn map_column(&mut self, generated: usize, original: usize) {
        self.column_map.insert(generated, original);
    }
    
    /// Get original line number for a generated line
    pub fn get_original_line(&self, generated_line: usize) -> Option<usize> {
        // Try exact match first
        if let Some(&orig) = self.line_map.get(&generated_line) {
            return Some(orig);
        }
        
        // Find nearest mapped line
        let mut nearest = None;
        let mut nearest_dist = usize::MAX;
        
        for (&gen, &orig) in &self.line_map {
            if gen <= generated_line {
                let dist = generated_line - gen;
                if dist < nearest_dist {
                    nearest_dist = dist;
                    nearest = Some(orig + dist);
                }
            }
        }
        
        nearest
    }
    
    /// Get original source line content
    pub fn get_source_line(&self, line_num: usize) -> Option<&str> {
        self.original_content.lines().nth(line_num.saturating_sub(1))
    }
    
    /// Format error with original source context
    pub fn format_error_context(&self, generated_line: usize, message: &str) -> String {
        let mut output = String::new();
        
        let orig_line = self.get_original_line(generated_line)
            .unwrap_or(generated_line);
        
        output.push_str(&format!(
            "error: {}\n  --> {}:{}\n",
            message,
            self.source_file.display(),
            orig_line
        ));
        
        // Show source context (3 lines before, target line, 2 lines after)
        let start = orig_line.saturating_sub(3);
        let end = orig_line + 2;
        
        for (i, line) in self.original_content.lines().enumerate() {
            let line_num = i + 1;
            if line_num >= start && line_num <= end {
                let marker = if line_num == orig_line { ">" } else { " " };
                output.push_str(&format!(
                    "{} {:4} | {}\n",
                    marker, line_num, line
                ));
            }
        }
        
        output
    }
}

/// Builder for source maps during code generation
#[derive(Debug, Default)]
pub struct SourceMapBuilder {
    current_source_line: usize,
    current_gen_line: usize,
    mappings: Vec<(usize, usize)>, // (generated, original)
}

impl SourceMapBuilder {
    pub fn new() -> Self {
        SourceMapBuilder {
            current_source_line: 1,
            current_gen_line: 1,
            mappings: Vec::new(),
        }
    }
    
    /// Called when processing a new source line
    pub fn advance_source(&mut self) {
        self.current_source_line += 1;
    }
    
    /// Called when emitting a generated line
    pub fn emit_line(&mut self) {
        self.mappings.push((self.current_gen_line, self.current_source_line));
        self.current_gen_line += 1;
    }
    
    /// Called when emitting multiple generated lines for one source line
    pub fn emit_lines(&mut self, count: usize) {
        for _ in 0..count {
            self.emit_line();
        }
    }
    
    /// Build the final source map
    pub fn build(self, source_file: PathBuf) -> SourceMap {
        let mut map = SourceMap::new(source_file);
        for (gen, orig) in self.mappings {
            map.map_line(gen, orig);
        }
        map
    }
}

/// Parse rustc error output and extract line/column information
#[derive(Debug, Clone)]
pub struct RustcError {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub error_code: Option<String>,
    pub notes: Vec<String>,
    pub help: Vec<String>,
}

pub fn parse_rustc_errors(stderr: &str) -> Vec<RustcError> {
    let mut errors = Vec::new();
    let mut current_error: Option<RustcError> = None;
    
    for line in stderr.lines() {
        // Match error format: error[E0425]: cannot find value `x` in this scope
        if line.starts_with("error") {
            // Save previous error
            if let Some(err) = current_error.take() {
                errors.push(err);
            }
            
            let (code, msg) = parse_error_header(line);
            current_error = Some(RustcError {
                file: String::new(),
                line: 0,
                column: 0,
                message: msg,
                error_code: code,
                notes: Vec::new(),
                help: Vec::new(),
            });
        }
        // Match location: --> src/main.rs:10:5
        else if line.trim().starts_with("-->") {
            if let Some(ref mut err) = current_error {
                if let Some((file, line_num, col)) = parse_location(line) {
                    err.file = file;
                    err.line = line_num;
                    err.column = col;
                }
            }
        }
        // Match note
        else if line.trim().starts_with("note:") {
            if let Some(ref mut err) = current_error {
                let note = line.trim().strip_prefix("note:").unwrap_or("").trim();
                err.notes.push(note.to_string());
            }
        }
        // Match help
        else if line.trim().starts_with("help:") {
            if let Some(ref mut err) = current_error {
                let help = line.trim().strip_prefix("help:").unwrap_or("").trim();
                err.help.push(help.to_string());
            }
        }
    }
    
    // Don't forget last error
    if let Some(err) = current_error {
        errors.push(err);
    }
    
    errors
}

fn parse_error_header(line: &str) -> (Option<String>, String) {
    // error[E0425]: message
    if let Some(bracket_start) = line.find('[') {
        if let Some(bracket_end) = line.find(']') {
            let code = line[bracket_start+1..bracket_end].to_string();
            let msg = line[bracket_end+1..].trim().trim_start_matches(':').trim().to_string();
            return (Some(code), msg);
        }
    }
    
    // error: message
    let msg = line.strip_prefix("error:").unwrap_or(line).trim().to_string();
    (None, msg)
}

fn parse_location(line: &str) -> Option<(String, usize, usize)> {
    // --> path/to/file.rs:line:column
    let trimmed = line.trim().strip_prefix("-->")?;
    let trimmed = trimmed.trim();
    
    let parts: Vec<&str> = trimmed.rsplitn(3, ':').collect();
    if parts.len() >= 3 {
        let column: usize = parts[0].parse().ok()?;
        let line_num: usize = parts[1].parse().ok()?;
        let file = parts[2..].join(":"); // Handle Windows paths with drive letter
        return Some((file, line_num, column));
    }
    
    None
}

/// Map rustc errors back to original RustS+ source
pub fn map_rustc_errors(
    errors: &[RustcError],
    source_map: &SourceMap,
) -> Vec<RustcError> {
    errors.iter().map(|err| {
        let orig_line = source_map.get_original_line(err.line)
            .unwrap_or(err.line);
        
        RustcError {
            file: source_map.source_file.to_string_lossy().to_string(),
            line: orig_line,
            column: err.column,
            message: err.message.clone(),
            error_code: err.error_code.clone(),
            notes: err.notes.clone(),
            help: err.help.clone(),
        }
    }).collect()
}

/// Format a mapped error for display
pub fn format_mapped_error(err: &RustcError, source_map: &SourceMap) -> String {
    let mut output = String::new();
    
    // Error header
    if let Some(ref code) = err.error_code {
        output.push_str(&format!("error[{}]: {}\n", code, err.message));
    } else {
        output.push_str(&format!("error: {}\n", err.message));
    }
    
    // Location
    output.push_str(&format!("  --> {}:{}:{}\n", err.file, err.line, err.column));
    
    // Source context
    output.push_str("   |\n");
    if let Some(line_content) = source_map.get_source_line(err.line) {
        output.push_str(&format!("{:3} | {}\n", err.line, line_content));
        output.push_str("   | ");
        
        // Underline the error location
        for _ in 0..err.column.saturating_sub(1) {
            output.push(' ');
        }
        output.push_str("^\n");
    }
    output.push_str("   |\n");
    
    // Notes
    for note in &err.notes {
        output.push_str(&format!("   = note: {}\n", note));
    }
    
    // Help
    for help in &err.help {
        output.push_str(&format!("   = help: {}\n", help));
    }
    
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_parse_error_header() {
        let (code, msg) = parse_error_header("error[E0425]: cannot find value `x`");
        assert_eq!(code, Some("E0425".to_string()));
        assert_eq!(msg, "cannot find value `x`");
        
        let (code, msg) = parse_error_header("error: some message");
        assert_eq!(code, None);
        assert_eq!(msg, "some message");
    }
    
    #[test]
    fn test_parse_location() {
        let loc = parse_location("  --> src/main.rs:10:5");
        assert_eq!(loc, Some(("src/main.rs".to_string(), 10, 5)));
        
        let loc = parse_location("  --> C:\\project\\src\\main.rs:20:3");
        assert!(loc.is_some());
    }
    
    #[test]
    fn test_source_map_builder() {
        let mut builder = SourceMapBuilder::new();
        
        // Simulate processing 3 source lines that become 5 generated lines
        builder.emit_line(); // gen 1 -> src 1
        builder.advance_source();
        builder.emit_lines(2); // gen 2,3 -> src 2
        builder.advance_source();
        builder.emit_lines(2); // gen 4,5 -> src 3
        
        let map = builder.build(PathBuf::from("test.rss"));
        
        assert_eq!(map.get_original_line(1), Some(1));
        assert_eq!(map.get_original_line(2), Some(2));
        assert_eq!(map.get_original_line(4), Some(3));
    }
}