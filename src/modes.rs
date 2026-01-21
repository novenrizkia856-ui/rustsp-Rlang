//! Mode tracking structures for RustS+ transpiler
//! 
//! Contains context tracking for different parsing modes:
//! - LiteralModeStack: Tracks struct/enum literal expressions
//! - ArrayModeStack: Tracks array literal expressions  
//! - UseImportMode: Tracks multi-line use import blocks

//===========================================================================
// LITERAL MODE CONTEXT
// Tracks when we are inside a struct/enum literal expression.
// In literal mode: NO `let`, NO `;`, ONLY field transformation.
//===========================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LiteralKind {
    Struct,      // Inside StructName { ... }
    EnumVariant, // Inside Enum::Variant { ... }
}

#[derive(Debug, Clone)]
pub struct LiteralModeEntry {
    pub kind: LiteralKind,
    pub start_depth: usize, // Brace depth when we entered
    pub is_assignment: bool, // true = `x = Struct {}`, false = bare `Struct {}` (return expr)
}

#[derive(Debug, Clone)]
pub struct LiteralModeStack {
    stack: Vec<LiteralModeEntry>,
}

impl LiteralModeStack {
    pub fn new() -> Self {
        LiteralModeStack { stack: Vec::new() }
    }
    
    pub fn enter(&mut self, kind: LiteralKind, depth: usize, is_assignment: bool) {
        self.stack.push(LiteralModeEntry { kind, start_depth: depth, is_assignment });
    }
    
    pub fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }
    
    #[allow(dead_code)]
    pub fn current_kind(&self) -> Option<LiteralKind> {
        self.stack.last().map(|e| e.kind)
    }
    
    pub fn current_is_assignment(&self) -> bool {
        self.stack.last().map(|e| e.is_assignment).unwrap_or(true)
    }
    
    /// Check if we should exit (depth went back to entry point)
    pub fn should_exit(&self, current_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            current_depth <= entry.start_depth
        } else {
            false
        }
    }
    
    pub fn exit(&mut self) {
        self.stack.pop();
    }
}

impl Default for LiteralModeStack {
    fn default() -> Self {
        Self::new()
    }
}

//===========================================================================
// ARRAY LITERAL MODE CONTEXT
// Tracks when we are inside an array literal expression: [elem1, elem2, ...]
// In array mode: NO semicolons inside, elements separated by commas.
// Array literals are ATOMIC - must be emitted as one complete expression.
//===========================================================================

#[derive(Debug, Clone)]
pub struct ArrayModeEntry {
    pub start_bracket_depth: usize, // Bracket depth when we entered
    pub is_assignment: bool,        // true = `x = [...]`, false = bare `[...]`
    pub var_name: String,           // Variable being assigned to
    pub var_type: Option<String>,   // Explicit type annotation if any
    pub needs_let: bool,            // Whether to emit `let`
    pub needs_mut: bool,            // Whether to emit `mut`
}

#[derive(Debug, Clone)]
pub struct ArrayModeStack {
    stack: Vec<ArrayModeEntry>,
}

impl ArrayModeStack {
    pub fn new() -> Self {
        ArrayModeStack { stack: Vec::new() }
    }
    
    pub fn enter(&mut self, bracket_depth: usize, is_assignment: bool, var_name: String, 
             var_type: Option<String>, needs_let: bool, needs_mut: bool) {
        self.stack.push(ArrayModeEntry { 
            start_bracket_depth: bracket_depth, 
            is_assignment,
            var_name,
            var_type,
            needs_let,
            needs_mut,
        });
    }
    
    pub fn is_active(&self) -> bool {
        !self.stack.is_empty()
    }
    
    pub fn current(&self) -> Option<&ArrayModeEntry> {
        self.stack.last()
    }
    
    /// Check if we should exit (bracket depth went back to entry point)
    pub fn should_exit(&self, current_bracket_depth: usize) -> bool {
        if let Some(entry) = self.stack.last() {
            current_bracket_depth <= entry.start_bracket_depth
        } else {
            false
        }
    }
    
    pub fn exit(&mut self) -> Option<ArrayModeEntry> {
        self.stack.pop()
    }
}

impl Default for ArrayModeStack {
    fn default() -> Self {
        Self::new()
    }
}

//===========================================================================
// USE IMPORT MODE CONTEXT
// Tracks when we are inside a multi-line use import block:
//   pub use module::{
//       Item1
//       Item2
//   }
// In use import mode: items get commas, closing } gets semicolon.
//===========================================================================

#[derive(Debug, Clone)]
pub struct UseImportMode {
    active: bool,
    start_brace_depth: usize, // Brace depth when we entered
    pub is_pub: bool,         // Whether it's `pub use` or just `use`
}

impl UseImportMode {
    pub fn new() -> Self {
        UseImportMode { 
            active: false, 
            start_brace_depth: 0,
            is_pub: false,
        }
    }
    
    pub fn enter(&mut self, brace_depth: usize, is_pub: bool) {
        self.active = true;
        self.start_brace_depth = brace_depth;
        self.is_pub = is_pub;
    }
    
    pub fn is_active(&self) -> bool {
        self.active
    }
    
    /// Check if we should exit (brace closes the use block)
    pub fn should_exit(&self, current_brace_depth: usize) -> bool {
        self.active && current_brace_depth <= self.start_brace_depth
    }
    
    pub fn exit(&mut self) {
        self.active = false;
    }
}

impl Default for UseImportMode {
    fn default() -> Self {
        Self::new()
    }
}

/// Check if line starts a multi-line use import block
/// Pattern: `use path::{` or `pub use path::{` where line ends with `{` 
/// but doesn't have a closing `}` on the same line
pub fn is_multiline_use_import_start(trimmed: &str) -> Option<bool> {
    let is_pub = trimmed.starts_with("pub use ");
    let is_use = trimmed.starts_with("use ");
    
    if !is_pub && !is_use {
        return None;
    }
    
    if !trimmed.contains('{') {
        return None;
    }
    
    // If has both `{` and `}`, it's a single-line import
    if trimmed.contains('}') {
        return None;
    }
    
    Some(is_pub)
}

/// Transform a use import item line - add comma if needed
pub fn transform_use_import_item(line: &str) -> String {
    let trimmed = line.trim();
    
    // Skip empty lines, comments, closing brace
    if trimmed.is_empty() || trimmed.starts_with("//") || trimmed == "}" {
        return line.to_string();
    }
    
    // Skip if already has comma or is the opening/closing
    if trimmed.ends_with(',') || trimmed.ends_with('{') || trimmed == "}" {
        return line.to_string();
    }
    
    // Add comma
    let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
    format!("{}{},", leading_ws, trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_literal_mode_stack() {
        let mut stack = LiteralModeStack::new();
        assert!(!stack.is_active());
        
        stack.enter(LiteralKind::Struct, 1, true);
        assert!(stack.is_active());
        assert!(stack.current_is_assignment());
        
        assert!(!stack.should_exit(2));
        assert!(stack.should_exit(1));
        
        stack.exit();
        assert!(!stack.is_active());
    }
    
    #[test]
    fn test_array_mode_stack() {
        let mut stack = ArrayModeStack::new();
        assert!(!stack.is_active());
        
        stack.enter(1, true, "arr".to_string(), None, true, false);
        assert!(stack.is_active());
        
        let current = stack.current().unwrap();
        assert_eq!(current.var_name, "arr");
        assert!(current.needs_let);
        
        stack.exit();
        assert!(!stack.is_active());
    }
    
    #[test]
    fn test_use_import_mode() {
        let mut mode = UseImportMode::new();
        assert!(!mode.is_active());
        
        mode.enter(1, true);
        assert!(mode.is_active());
        assert!(mode.is_pub);
        
        assert!(mode.should_exit(1));
        assert!(!mode.should_exit(2));
        
        mode.exit();
        assert!(!mode.is_active());
    }
    
    #[test]
    fn test_is_multiline_use_import_start() {
        assert_eq!(is_multiline_use_import_start("pub use foo::{"), Some(true));
        assert_eq!(is_multiline_use_import_start("use foo::{"), Some(false));
        assert_eq!(is_multiline_use_import_start("use foo::{Bar}"), None); // single line
        assert_eq!(is_multiline_use_import_start("let x = 10"), None);
    }
    
    #[test]
    fn test_transform_use_import_item() {
        assert_eq!(transform_use_import_item("    ItemName"), "    ItemName,");
        assert_eq!(transform_use_import_item("    ItemName,"), "    ItemName,");
        assert_eq!(transform_use_import_item("    }"), "    }");
    }
}