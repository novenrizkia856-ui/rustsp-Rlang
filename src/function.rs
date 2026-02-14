//! Function & Parameter Grammar for RustS+
//!
//! Syntax transformations:
//! - RustS+: `fn add(a i32, b i32) i32 { body }` → Rust: `fn add(a: i32, b: i32) -> i32 { body }`
//! - RustS+: `fn add(a i32, b i32) i32 = expr` → Rust: `fn add(a: i32, b: i32) -> i32 { expr }`
//! - RustS+: `fn id[T](x T) T { x }` → Rust: `fn id<T>(x: T) -> T { x }`
//! - Borrow: `fn read(x &String)` → `fn read(x: &String)`
//! - Rust passthrough: `fn foo(a: i32) -> i32` remains unchanged
//!
//! Expression transformations:
//! - String concat: `&String + &str` → `lhs.to_owned() + ...`
//! - Call coercion: `foo("lit")` where param is &String → `foo(&String::from("lit"))`
//! - Tail return: last expr in non-() function has no semicolon

use std::collections::HashMap;

/// A parsed function parameter
#[derive(Debug, Clone)]
pub struct Parameter {
    pub name: String,
    pub param_type: String,
    pub is_borrow: bool,
    pub is_mut_borrow: bool,
    /// Parameter has explicit `mut` modifier (e.g., `mut get_balance F1`)
    pub is_mut_param: bool,
}

/// A parsed function signature
#[derive(Debug, Clone)]
pub struct FunctionSignature {
    pub name: String,
    pub generics: Option<String>,
    pub parameters: Vec<Parameter>,
    pub return_type: Option<String>,
    pub is_pub: bool,
    pub is_single_line: bool,
    pub single_line_expr: Option<String>,
    /// Parameters that have `write` effect - these need `mut` in Rust output
    pub write_params: Vec<String>,
}

/// Result of parsing a function line
#[derive(Debug)]
pub enum FunctionParseResult {
    RustSPlusSignature(FunctionSignature),
    RustPassthrough,
    NotAFunction,
    Error(String),
}

// ============================================================================
// PARAMETER TYPE TRANSFORMATION
// ============================================================================

/// Transform RustS+ generic syntax to Rust generic syntax
/// RustS+ uses square brackets for generics: `Vec[String]`, `HashMap[K, V]`
/// Rust uses angle brackets: `Vec<String>`, `HashMap<K, V>`
/// 
/// Also handles lifetime parameters: `Formatter[_]` → `Formatter<'_>`
fn transform_generic_brackets(type_str: &str) -> String {
    let trimmed = type_str.trim();
    
    // List of known generic types that use Type[T] syntax
    // CRITICAL: Includes both container types AND traits with generic parameters
    const GENERIC_TYPES: &[&str] = &[
        // Collections
        "Vec", "HashMap", "HashSet", "BTreeMap", "BTreeSet",
        "VecDeque", "LinkedList", "BinaryHeap",
        // Smart pointers & wrappers
        "Option", "Result", "Box", "Rc", "Arc", "RefCell", "Cell",
        "Mutex", "RwLock", "Cow", "PhantomData", "Weak", "Pin",
        // Conversion traits (CRITICAL for impl Into[T], From[T], etc.)
        "Into", "From", "TryInto", "TryFrom",
        "AsRef", "AsMut",
        // Iterator traits
        "Iterator", "IntoIterator", "ExactSizeIterator", "DoubleEndedIterator",
        // Function traits
        "Fn", "FnMut", "FnOnce", "FnPtr",
        // Deref/Borrow traits
        "Deref", "DerefMut", "Borrow", "BorrowMut",
        // Range types
        "Range", "RangeInclusive", "RangeFrom", "RangeTo", "RangeFull",
        // CRITICAL: Async/Future types for async trait methods
        "Future", "Stream", "Poll",
        // std::fmt types (CRITICAL for Display/Debug implementations)
        "Formatter", "Arguments",
        // Other common generics
        "Sender", "Receiver", "SyncSender",
        "MaybeUninit", "ManuallyDrop",
        // Serde traits
        "Serialize", "Deserialize",
        // Common external crates
        "Lazy", "OnceCell", "OnceLock",
    ];
    
    // CRITICAL: Types that take LIFETIME parameters instead of type parameters
    // When inner content is `_`, it must become `'_` (lifetime elision placeholder)
    const LIFETIME_PARAM_TYPES: &[&str] = &[
        "Formatter",   // std::fmt::Formatter<'a>
        "Arguments",   // std::fmt::Arguments<'a>
    ];
    
    let mut result = trimmed.to_string();
    
    // CRITICAL FIX: Loop until no more transformations are needed
    // This ensures ALL occurrences of each generic type are transformed
    // Bug fix: The old code only found the FIRST occurrence of each pattern
    let mut changed = true;
    while changed {
        changed = false;
        
        for generic_type in GENERIC_TYPES {
            let pattern = format!("{}[", generic_type);
            
            if let Some(pos) = result.find(&pattern) {
                let is_word_boundary = pos == 0 || {
                    let prev_char = result.chars().nth(pos - 1).unwrap_or(' ');
                    !prev_char.is_alphanumeric() && prev_char != '_'
                };
                
                if is_word_boundary {
                    let bracket_start = pos + generic_type.len();
                    if let Some(bracket_end) = find_matching_bracket(&result[bracket_start..]) {
                        let inner = &result[bracket_start + 1..bracket_start + bracket_end];
                        let mut transformed_inner = transform_generic_brackets(inner);
                        
                        // CRITICAL FIX: Handle lifetime parameter types
                        // For types like Formatter that take lifetimes, `_` must become `'_`
                        if LIFETIME_PARAM_TYPES.contains(generic_type) {
                            // If inner is just `_`, convert to lifetime placeholder `'_`
                            if transformed_inner.trim() == "_" {
                                transformed_inner = "'_".to_string();
                            }
                        }
                        
                        let before = &result[..pos];
                        let after = &result[bracket_start + bracket_end + 1..];
                        
                        result = format!("{}{}<{}>{}", before, generic_type, transformed_inner, after);
                        changed = true;
                        break; // Restart loop
                    }
                }
            }
        }
    }
    
    result
}

fn find_matching_bracket(s: &str) -> Option<usize> {
    if !s.starts_with('[') {
        return None;
    }
    
    let mut depth = 0;
    for (i, c) in s.chars().enumerate() {
        match c {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// L-06: Transform parameter type for valid Rust
/// 
/// Bare slice types [T] are unsized and cannot be function parameters.
/// They must be transformed to &[T] (borrowed slice).
/// 
/// L-05: Also strips any effect annotations from parameter types!
/// Effect annotations MUST NOT appear in Rust output.
/// 
/// Also transforms RustS+ generic syntax: Vec[T] → Vec<T>
/// Also transforms function pointer types: fn(T) R → fn(T) -> R
/// 
/// Examples:
/// - `[Tx]` → `&[Tx]`
/// - `[u8]` → `&[u8]`
/// - `Vec[T]` → `Vec<T>` (generic transformation)
/// - `Vec<T>` → `Vec<T>` (unchanged, already Rust syntax)
/// - `&[T]` → `&[T]` (already borrowed, unchanged)
/// - `effects(read x) T` → `T` (effects stripped)
/// - `fn(Account) Account` → `fn(Account) -> Account` (function pointer)
fn transform_param_type(type_str: &str) -> String {
    // L-05 CRITICAL: Strip effects clause FIRST before any other transformation
    let stripped = strip_effects_clause(type_str);
    let trimmed = stripped.trim();
    
    // CRITICAL: Transform generic brackets Vec[T] → Vec<T>
    let generic_transformed = transform_generic_brackets(trimmed);
    let trimmed = generic_transformed.as_str();
    
    // CRITICAL: Transform function pointer types fn(T) R → fn(T) -> R
    let fn_transformed = transform_fn_pointer_type(trimmed);
    let trimmed = fn_transformed.as_str();
    
    // If it's already a reference, leave it alone
    if trimmed.starts_with('&') {
        return trimmed.to_string();
    }
    
    // If it's a bare slice type [T], convert to &[T]
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        return format!("&{}", trimmed);
    }
    
    // Otherwise return as-is
    trimmed.to_string()
}

// ============================================================================
// FUNCTION REGISTRY - Tracks function signatures for call-site coercion
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct FunctionRegistry {
    functions: HashMap<String, FunctionSignature>,
}

impl FunctionRegistry {
    pub fn new() -> Self {
        FunctionRegistry { functions: HashMap::new() }
    }
    
    pub fn register(&mut self, sig: FunctionSignature) {
        // L-10: Transform parameter types before storing in registry
        // This ensures call-site coercion works correctly with transformed types
        let mut transformed_sig = sig.clone();
        for param in &mut transformed_sig.parameters {
            param.param_type = transform_param_type(&param.param_type);
        }
        self.functions.insert(transformed_sig.name.clone(), transformed_sig);
    }
    
    pub fn get(&self, name: &str) -> Option<&FunctionSignature> {
        self.functions.get(name)
    }
}

// ============================================================================
// CURRENT FUNCTION CONTEXT
// ============================================================================

#[derive(Debug, Clone, Default)]
pub struct CurrentFunctionContext {
    pub name: Option<String>,
    pub params: HashMap<String, String>,
    pub return_type: Option<String>,
    pub start_depth: usize,
}

impl CurrentFunctionContext {
    pub fn new() -> Self {
        CurrentFunctionContext {
            name: None,
            params: HashMap::new(),
            return_type: None,
            start_depth: 0,
        }
    }
    
    pub fn enter(&mut self, sig: &FunctionSignature, depth: usize) {
        self.name = Some(sig.name.clone());
        self.params.clear();
        for p in &sig.parameters {
            // CRITICAL FIX: Store TRANSFORMED param type, not raw type
            // This ensures is_slice_param() works correctly for RustS+ [T] → &[T]
            let transformed_type = transform_param_type(&p.param_type);
            self.params.insert(p.name.clone(), transformed_type);
        }
        self.return_type = sig.return_type.clone();
        self.start_depth = depth;
    }
    
    pub fn exit(&mut self) {
        self.name = None;
        self.params.clear();
        self.return_type = None;
        self.start_depth = 0;
    }
    
    pub fn is_inside(&self) -> bool {
        self.name.is_some()
    }
    
    pub fn has_return_value(&self) -> bool {
        self.return_type.is_some()
    }
    
    pub fn is_ref_string_param(&self, name: &str) -> bool {
        self.params.get(name)
            .map(|t| t == "&String" || t == "&mut String")
            .unwrap_or(false)
    }
    
    /// Check if a parameter is a slice type (&[T])
    /// This is used to know when to add .to_vec() for struct field assignments
    pub fn is_slice_param(&self, name: &str) -> bool {
        self.params.get(name)
            .map(|t| t.starts_with("&[") && t.ends_with(']'))
            .unwrap_or(false)
    }
    
    /// Get the parameter type for a given name
    pub fn get_param_type(&self, name: &str) -> Option<&String> {
        self.params.get(name)
    }
}

// ============================================================================
// STRING CONCATENATION TRANSFORMER
// ============================================================================

pub fn transform_string_concat(expr: &str, ctx: &CurrentFunctionContext) -> String {
    let expr = expr.trim();
    if !expr.contains('+') {
        return expr.to_string();
    }
    
    let parts = split_by_plus(expr);
    if parts.len() < 2 {
        return expr.to_string();
    }
    
    let has_string_literal = parts.iter().any(|p| {
        let t = p.trim();
        t.starts_with('"') || t.starts_with("&\"")
    });
    
    let first_part = parts[0].trim();
    let first_is_ref_string = ctx.is_ref_string_param(first_part);
    
    if !has_string_literal && !first_is_ref_string {
        return expr.to_string();
    }
    
    let mut result_parts: Vec<String> = Vec::new();
    for (i, part) in parts.iter().enumerate() {
        let part = part.trim();
        if i == 0 && ctx.is_ref_string_param(part) {
            result_parts.push(format!("{}.to_owned()", part));
        } else {
            result_parts.push(part.to_string());
        }
    }
    
    result_parts.join(" + ")
}

fn split_by_plus(expr: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for c in expr.chars() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '(' | '[' | '{' => { depth += 1; current.push(c); }
                ')' | ']' | '}' => { depth -= 1; current.push(c); }
                '+' if depth == 0 => {
                    parts.push(current.trim().to_string());
                    current = String::new();
                    prev = c;
                    continue;
                }
                _ => current.push(c),
            }
        } else {
            current.push(c);
        }
        prev = c;
    }
    
    if !current.trim().is_empty() {
        parts.push(current.trim().to_string());
    }
    parts
}

// ============================================================================
// CALL ARGUMENT COERCION
// ============================================================================

pub fn transform_call_args(line: &str, registry: &FunctionRegistry) -> String {
    transform_call_args_with_ctx(line, registry, None)
}

pub fn transform_call_args_with_ctx(
    line: &str,
    registry: &FunctionRegistry,
    current_fn_ctx: Option<&CurrentFunctionContext>,
) -> String {
    let line = line.trim();
    if !line.contains('(') {
        return line.to_string();
    }
    
    let mut result = line.to_string();
    
    if let Some((func_name, paren_pos)) = find_function_call(line) {
        if let Some(sig) = registry.get(&func_name) {
            if let Some(close_paren) = find_matching_paren_from(line, paren_pos) {
                let before = &line[..paren_pos - func_name.len()];
                let args_str = &line[paren_pos + 1..close_paren];
                let after = &line[close_paren + 1..];
                
                let args = split_call_args(args_str);
                let mut new_args = Vec::new();
                
                for (i, arg) in args.iter().enumerate() {
                    let arg = arg.trim();
                    if let Some(param) = sig.parameters.get(i) {
                        new_args.push(coerce_argument(arg, &param.param_type, current_fn_ctx));
                    } else {
                        new_args.push(arg.to_string());
                    }
                }
                
                result = format!("{}{}({}){}", before, func_name, new_args.join(", "), after);
            }
        }
    }
    
    result
}

fn find_function_call(expr: &str) -> Option<(String, usize)> {
    let chars: Vec<char> = expr.chars().collect();
    let mut i = 0;
    
    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let name: String = chars[start..i].iter().collect();
            
            if i < chars.len() && chars[i] == '(' {
                if !matches!(name.as_str(), "if" | "while" | "for" | "match" | "let" | "return" | "println" | "print" | "eprintln" | "format" | "vec" | "panic" | "assert") {
                    return Some((name, i));
                }
            }
        } else {
            i += 1;
        }
    }
    None
}

fn find_matching_paren_from(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for (i, c) in s[start..].char_indices() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(start + i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

fn split_call_args(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for c in s.chars() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        
        if !in_string {
            match c {
                '(' | '[' | '{' | '<' => { depth += 1; current.push(c); }
                ')' | ']' | '}' | '>' => { depth -= 1; current.push(c); }
                ',' if depth == 0 => {
                    result.push(current.trim().to_string());
                    current = String::new();
                    prev = c;
                    continue;
                }
                _ => current.push(c),
            }
        } else {
            current.push(c);
        }
        prev = c;
    }
    
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn coerce_argument(arg: &str, param_type: &str, current_fn_ctx: Option<&CurrentFunctionContext>) -> String {
    let arg = arg.trim();
    
    // CRITICAL FIX: Transform param_type first (e.g., [T] → &[T])
    // The signature stores original RustS+ types, but we need Rust types for coercion
    let transformed_param_type = transform_param_type(param_type);
    let param_type = transformed_param_type.as_str();
    
    // L-10: Handle &[T] parameters - add & to array arguments
    // When param is &[T] and arg is a plain identifier (array variable), add &
    if param_type.starts_with("&[") {
        // Borrow hygiene: if argument is already known as a reference type, pass directly.
        if let Some(ctx) = current_fn_ctx {
            if is_simple_identifier(arg) {
                if let Some(arg_ty) = ctx.params.get(arg) {
                    if arg_ty.trim_start().starts_with('&') {
                        return arg.to_string();
                    }
                }
            }
        }

        // Check if arg is an array literal like [x, y, z]
        if arg.starts_with('[') && arg.ends_with(']') && !arg.starts_with("&[") {
            // Array literal needs & to convert to slice reference
            return format!("&{}", arg);
        }
        // Check if arg is already a reference
        if !arg.starts_with('&') {
            // Check if it's a simple identifier (array variable)
            // Not a complex expression like function call or method
            if is_simple_identifier(arg) {
                return format!("&{}", arg);
            }
        }
        return arg.to_string();
    }
    
    // L-11: Handle slice indexing - add .clone() when needed
    // When arg is slice[index] and param expects owned value (not &T), add .clone()
    if is_slice_index_access(arg) && !param_type.starts_with('&') {
        // Add .clone() for slice index access passed by value
        return format!("{}.clone()", arg);
    }
    
    if param_type == "&String" {
        if arg.starts_with('"') && arg.ends_with('"') {
            return format!("&String::from({})", arg);
        }
        if arg.starts_with("&\"") && arg.ends_with('"') {
            let inner = &arg[1..];
            return format!("&String::from({})", inner);
        }
        return arg.to_string();
    }
    
    if param_type == "String" {
        if arg.starts_with('"') && arg.ends_with('"') {
            return format!("String::from({})", arg);
        }
    }
    
    // ==========================================================================
    // CRITICAL FIX: Auto-clone struct arguments
    // RustS+ uses value semantics - when passing a struct to a function,
    // we want to clone it so the original can still be used.
    // This prevents all "use of moved value" errors.
    // ==========================================================================
    
    // Check if param_type is a struct (non-primitive, non-reference)
    if should_auto_clone_for_param(param_type) && should_clone_argument(arg) {
        return format!("{}.clone()", arg);
    }
    
    arg.to_string()
}

/// Check if a parameter type requires auto-clone (is a struct/non-Copy type)
fn should_auto_clone_for_param(param_type: &str) -> bool {
    let pt = param_type.trim();
    
    // Skip if already a reference
    if pt.starts_with('&') {
        return false;
    }
    
    // Skip primitive Copy types
    let primitives = [
        "u8", "u16", "u32", "u64", "u128", "usize",
        "i8", "i16", "i32", "i64", "i128", "isize",
        "f32", "f64", "bool", "char", "()",
    ];
    
    if primitives.contains(&pt) {
        return false;
    }
    
    // Skip generic/slice types that start with &
    if pt.starts_with("&[") || pt.starts_with("&str") {
        return false;
    }
    
    // If it starts with uppercase letter, it's likely a struct/enum type
    if let Some(first_char) = pt.chars().next() {
        if first_char.is_uppercase() {
            return true;
        }
    }
    
    false
}

/// Check if an argument should have .clone() added
fn should_clone_argument(arg: &str) -> bool {
    let a = arg.trim();
    
    // Skip if empty
    if a.is_empty() {
        return false;
    }
    
    // Skip if already has .clone()
    if a.ends_with(".clone()") {
        return false;
    }
    
    // Skip if it's a literal
    if a.starts_with('"') || a.starts_with('\'') {
        return false;
    }
    if a.parse::<i64>().is_ok() || a.parse::<f64>().is_ok() {
        return false;
    }
    if a == "true" || a == "false" {
        return false;
    }
    
    // Skip if it's already a reference
    if a.starts_with('&') {
        return false;
    }
    
    // Skip if it's a function call (ends with `)` but not `.clone()`)
    // We don't want to clone function results - they're already owned
    if a.ends_with(')') && !a.ends_with(".clone()") {
        // Check if it's a method call vs function call
        // Method calls on values should still be cloned in some cases
        // But function calls return owned values, no need to clone
        if !a.contains('.') {
            return false;  // Pure function call like func()
        }
        // For method calls like x.method(), only skip if it returns a new value
        // This is hard to determine without type info, so skip all method calls
        return false;
    }
    
    // Clone simple identifiers and field accesses
    // These are likely struct values that could be moved
    if is_simple_identifier(a) || is_field_access(a) {
        return true;
    }
    
    false
}

/// Check if expression is a field access like `x.field` or `x.y.z`
fn is_field_access(s: &str) -> bool {
    let trimmed = s.trim();
    
    // Must contain dot but not be a method call
    if !trimmed.contains('.') {
        return false;
    }
    
    // Must not end with () - that's a method call
    if trimmed.ends_with(')') {
        return false;
    }
    
    // Must not contain :: - that's a path
    if trimmed.contains("::") {
        return false;
    }
    
    // Split by dots and check each part is a valid identifier
    let parts: Vec<&str> = trimmed.split('.').collect();
    if parts.len() < 2 {
        return false;
    }
    
    // Each part should be a valid identifier
    for part in parts {
        if !is_simple_identifier(part) {
            return false;
        }
    }
    
    true
}

/// Check if a string is a simple identifier (variable name)
/// Not a function call, method call, or complex expression
fn is_simple_identifier(s: &str) -> bool {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return false;
    }
    
    // Must start with letter or underscore
    let first_char = trimmed.chars().next().unwrap();
    if !first_char.is_alphabetic() && first_char != '_' {
        return false;
    }
    
    // Must be alphanumeric or underscore only
    // No parens, brackets, dots, etc.
    trimmed.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Check if a string is a slice/array index access pattern: `var[index]`
fn is_slice_index_access(s: &str) -> bool {
    let trimmed = s.trim();
    
    // Must contain [ and end with ]
    if !trimmed.contains('[') || !trimmed.ends_with(']') {
        return false;
    }
    
    // Find the bracket position
    if let Some(bracket_pos) = trimmed.find('[') {
        // Part before [ must be a valid base (identifier or field access)
        let var_part = &trimmed[..bracket_pos];
        
        // CRITICAL FIX: Accept both simple identifiers AND field access patterns
        // Examples:
        // - `arr[i]` - simple identifier
        // - `block.transactions[i]` - field access
        // - `self.data[idx]` - field access
        if is_simple_identifier(var_part) || is_valid_index_base(var_part) {
            // Check that brackets are balanced and at the end
            let bracket_part = &trimmed[bracket_pos..];
            let open_count = bracket_part.chars().filter(|&c| c == '[').count();
            let close_count = bracket_part.chars().filter(|&c| c == ']').count();
            return open_count == close_count;
        }
    }
    
    false
}

/// Check if expression is a valid base for array indexing
/// Accepts: simple identifiers, field access chains
/// Examples: `arr`, `block.transactions`, `self.data.items`
fn is_valid_index_base(s: &str) -> bool {
    let trimmed = s.trim();
    
    // Must not be empty
    if trimmed.is_empty() {
        return false;
    }
    
    // Must not contain brackets or parens
    if trimmed.contains('[') || trimmed.contains('(') || trimmed.contains(')') {
        return false;
    }
    
    // Must not contain :: (path separator)
    if trimmed.contains("::") {
        return false;
    }
    
    // If it contains dots, it's a field access chain
    if trimmed.contains('.') {
        // Split by dots and verify each part is a valid identifier
        let parts: Vec<&str> = trimmed.split('.').collect();
        if parts.len() < 2 {
            return false;
        }
        
        // Each part must be a valid identifier
        for part in parts {
            let p = part.trim();
            if p.is_empty() {
                return false;
            }
            if !is_simple_identifier(p) {
                return false;
            }
        }
        return true;
    }
    
    // Otherwise it should be a simple identifier
    is_simple_identifier(trimmed)
}

// ============================================================================
// TAIL RETURN DETECTION
// ============================================================================

pub fn should_be_tail_return(
    line: &str,
    ctx: &CurrentFunctionContext,
    is_before_closing_brace: bool,
) -> bool {
    if !ctx.has_return_value() || !is_before_closing_brace {
        return false;
    }
    
    let trimmed = line.trim();
    
    if trimmed.ends_with(';') {
        return false;
    }
    
    if trimmed.starts_with("let ")
        || trimmed.starts_with("if ")
        || trimmed.starts_with("while ")
        || trimmed.starts_with("for ")
        || trimmed.starts_with("match ")
        || trimmed.starts_with("return ")
        || trimmed.contains("println!")
        || trimmed.contains("print!")
        || trimmed.contains("eprintln!")
        || trimmed.contains("panic!")
        || trimmed.contains("assert!")
    {
        return false;
    }
    
    if trimmed.contains(".push(")
        || trimmed.contains(".insert(")
        || trimmed.contains(".remove(")
        || trimmed.contains(".clear(")
    {
        return false;
    }

    // CRITICAL: Assignments (including array/field assignments) must NOT be treated
    // as tail returns. Otherwise `arr[i] = x` becomes a tail expression and can
    // silently change control flow or trigger type errors for non-() return types.
    if has_standalone_assignment_eq(trimmed) {
        return false;
    }
    
    true
}

/// Detect a standalone assignment `=` at top level (not comparison or compound op).
fn has_standalone_assignment_eq(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let len = chars.len();
    let mut paren_depth: usize = 0;
    let mut bracket_depth: usize = 0;
    let mut brace_depth: usize = 0;
    let mut in_string = false;
    let mut prev_char = ' ';

    for i in 0..len {
        let c = chars[i];

        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
            prev_char = c;
            continue;
        }

        if in_string {
            prev_char = c;
            continue;
        }

        match c {
            '(' => paren_depth += 1,
            ')' => paren_depth = paren_depth.saturating_sub(1),
            '[' => bracket_depth += 1,
            ']' => bracket_depth = bracket_depth.saturating_sub(1),
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }

        if c == '=' && paren_depth == 0 && bracket_depth == 0 && brace_depth == 0 {
            let prev = if i > 0 { chars[i - 1] } else { ' ' };
            let next = if i + 1 < len { chars[i + 1] } else { ' ' };

            if next == '=' || next == '>' {
                prev_char = c;
                continue;
            }
            if prev == '=' || prev == '!' || prev == '<' || prev == '>'
                || prev == '+' || prev == '-' || prev == '*' || prev == '/' || prev == '%'
                || prev == '&' || prev == '|' || prev == '^'
            {
                prev_char = c;
                continue;
            }
            if i >= 2 {
                let prev_prev = chars[i - 2];
                if (prev == '<' && prev_prev == '<') || (prev == '>' && prev_prev == '>') {
                    prev_char = c;
                    continue;
                }
            }

            return true;
        }

        prev_char = c;
    }

    false
}

// ============================================================================
// PARSING FUNCTIONS
// ============================================================================

pub fn parse_function_line(line: &str) -> FunctionParseResult {
    let trimmed = line.trim();
    
    let (is_pub, fn_part) = if trimmed.starts_with("pub fn ") {
        (true, trimmed.strip_prefix("pub fn ").unwrap())
    } else if trimmed.starts_with("fn ") {
        (false, trimmed.strip_prefix("fn ").unwrap())
    } else {
        return FunctionParseResult::NotAFunction;
    };
    
    if is_rust_syntax(trimmed) {
        return FunctionParseResult::RustPassthrough;
    }
    
    match parse_rustsplus_function(fn_part, is_pub) {
        Ok(sig) => FunctionParseResult::RustSPlusSignature(sig),
        Err(e) => FunctionParseResult::Error(e),
    }
}

fn is_rust_syntax(line: &str) -> bool {
    // L-05 CRITICAL FIX: If the line contains "effects(" outside of strings/comments,
    // it's NOT pure Rust syntax - it needs transformation to strip effects!
    // Check for effects BEFORE determining if it's Rust syntax
    if line.contains("effects(") {
        // Quick check: make sure it's not in a string or comment
        let mut in_string = false;
        let mut prev_char = ' ';
        let chars: Vec<char> = line.chars().collect();
        
        for (i, &c) in chars.iter().enumerate() {
            if c == '"' && prev_char != '\\' {
                in_string = !in_string;
            }
            prev_char = c;
            
            // Check for "effects(" pattern outside of strings
            if !in_string && i + 8 <= chars.len() {
                let slice: String = chars[i..i+8].iter().collect();
                if slice == "effects(" {
                    // Found effects annotation outside string - NOT pure Rust
                    return false;
                }
            }
        }
    }
    
    // Check parameter syntax - if params contain ": " it's Rust syntax
    if let Some(paren_start) = line.find('(') {
        if let Some(paren_end) = find_matching_paren(line, paren_start) {
            let params = &line[paren_start..=paren_end];
            // If params have colon-space, it's Rust syntax
            if params.contains(": ") {
                return true;
            }
            // Empty params () could be either - check for -> for determination
            // But if params exist without `:`, it's RustS+ syntax
            let inner = &params[1..params.len()-1].trim();
            if !inner.is_empty() && !params.contains(':') {
                // Non-empty params without colon = RustS+ syntax
                return false;
            }
        }
    }
    // No params with colons - check if it looks like pure Rust
    // But only if there are NO params at all or empty params
    false
}

fn find_matching_paren(s: &str, start: usize) -> Option<usize> {
    let mut depth = 0;
    let mut in_string = false;
    let mut prev = ' ';
    
    for (i, c) in s[start..].char_indices() {
        if c == '"' && prev != '\\' {
            in_string = !in_string;
        }
        if !in_string {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(start + i);
                    }
                }
                _ => {}
            }
        }
        prev = c;
    }
    None
}

/// Strip effects clause from return type
/// 
/// L-05 FIX: Effect annotations must NOT appear in Rust output.
/// 
/// RustS+ allows: `fn f(x T) effects(write x) U { ... }`
/// Rust output must be: `fn f(x: T) -> U { ... }`
/// 
/// This function strips `effects(...)` from the type string.
/// 
/// Examples:
/// - `effects(write x) Wallet` → `Wallet`
/// - `effects(io) ()` → `()`
/// - `effects(read a, write b) Result<T, E>` → `Result<T, E>`
/// - `String` → `String` (no effects, unchanged)
pub fn strip_effects_clause(type_str: &str) -> String {
    let trimmed = type_str.trim();
    
    // Check if starts with "effects("
    if !trimmed.starts_with("effects(") {
        // CRITICAL FIX: Also strip leading `->` if present
        // This handles cases like "-> Block" that should just be "Block"
        if trimmed.starts_with("->") {
            return trimmed[2..].trim().to_string();
        }
        return trimmed.to_string();
    }
    
    // Find the matching closing paren for effects(...)
    // Need to handle nested parens like effects(write x) Result<T, (A, B)>
    let mut paren_depth = 0;
    let mut effects_end = 0;
    
    for (i, c) in trimmed.char_indices() {
        match c {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    effects_end = i + 1;
                    break;
                }
            }
            _ => {}
        }
    }
    
    if effects_end == 0 {
        // Malformed effects clause - return as-is (will be caught by sanity check)
        return trimmed.to_string();
    }
    
    // Return everything after the effects(...) clause, trimmed
    // CRITICAL FIX: Also strip leading `->` from the result
    // `effects(alloc) -> Block` should return `Block`, not `-> Block`
    let result = trimmed[effects_end..].trim();
    if result.starts_with("->") {
        result[2..].trim().to_string()
    } else {
        result.to_string()
    }
}

/// Extract parameters with `write` effect from effects clause
/// 
/// Examples:
/// - `effects(write acc)` → ["acc"]
/// - `effects(write from, write to, io)` → ["from", "to"]
/// - `effects(io, alloc)` → []
/// - `effects(write(acc))` → ["acc"]  (parenthesized form)
/// - `effects(write self)` → ["self"]
pub fn extract_write_params(type_str: &str) -> Vec<String> {
    let trimmed = type_str.trim();
    
    if !trimmed.starts_with("effects(") {
        return Vec::new();
    }
    
    // Find the matching closing paren for effects(...)
    let mut paren_depth = 0;
    let mut effects_end = 0;
    
    for (i, c) in trimmed.char_indices() {
        match c {
            '(' => paren_depth += 1,
            ')' => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    effects_end = i;
                    break;
                }
            }
            _ => {}
        }
    }
    
    if effects_end == 0 {
        return Vec::new();
    }
    
    // Extract the content inside effects(...)
    let effects_content = &trimmed[8..effects_end]; // Skip "effects("
    
    let mut write_params = Vec::new();
    
    // Split by comma and look for "write" effects
    for effect in effects_content.split(',') {
        let effect = effect.trim();
        
        // Handle both forms:
        // - "write acc" (space-separated)
        // - "write(acc)" (parenthesized)
        if effect.starts_with("write(") && effect.ends_with(')') {
            // Parenthesized form: write(acc)
            let param = effect[6..effect.len()-1].trim();
            if !param.is_empty() {
                write_params.push(param.to_string());
            }
        } else if effect.starts_with("write ") {
            // Space-separated form: write acc
            let param = effect[6..].trim();
            if !param.is_empty() {
                write_params.push(param.to_string());
            }
        }
    }
    
    write_params
}

/// Transform function pointer types: fn(T) R → fn(T) -> R
/// Also handles multiple params: fn(A, B) C → fn(A, B) -> C
fn transform_fn_pointer_type(type_str: &str) -> String {
    let trimmed = type_str.trim();
    
    // Must start with "fn("
    if !trimmed.starts_with("fn(") {
        return trimmed.to_string();
    }
    
    // Find the closing paren
    let mut depth = 0;
    let mut paren_end = None;
    
    for (i, c) in trimmed.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    paren_end = Some(i);
                    break;
                }
            }
            _ => {}
        }
    }
    
    let paren_end = match paren_end {
        Some(pos) => pos,
        None => return trimmed.to_string(),
    };
    
    // Get the part after the closing paren
    let after_paren = trimmed[paren_end + 1..].trim();
    
    // If it's empty or already has ->, return as-is
    if after_paren.is_empty() || after_paren.starts_with("->") {
        return trimmed.to_string();
    }
    
    // There's a return type without -> , add it
    let fn_params = &trimmed[..paren_end + 1];
    format!("{} -> {}", fn_params, after_paren)
}

fn parse_rustsplus_function(after_fn: &str, is_pub: bool) -> Result<FunctionSignature, String> {
    let mut rest = after_fn.trim();
    
    let name_end = rest.find(|c: char| c == '(' || c == '[' || c.is_whitespace())
        .ok_or("Invalid function: missing name")?;
    let name = rest[..name_end].to_string();
    rest = rest[name_end..].trim();
    
    if name.is_empty() {
        return Err("Function name cannot be empty".to_string());
    }
    
    let generics = if rest.starts_with('[') {
        let bracket_end = rest.find(']').ok_or("Invalid generics: missing ']'")?;
        let gen = rest[1..bracket_end].trim().to_string();
        rest = rest[bracket_end + 1..].trim();
        Some(gen)
    } else {
        None
    };
    
    if !rest.starts_with('(') {
        return Err("Invalid function: expected '(' after name".to_string());
    }
    
    let paren_end = find_matching_paren(rest, 0).ok_or("Invalid function: unmatched '('")?;
    let params_str = &rest[1..paren_end];
    rest = rest[paren_end + 1..].trim();
    
    let parameters = parse_parameters(params_str)?;
    
    // CRITICAL FIX: Extract write params BEFORE stripping effects clause
    // This allows us to know which params need `mut` in the Rust output
    let write_params = extract_write_params(rest);
    
    let (return_type, is_single_line, single_line_expr) = if rest.is_empty() || rest == "{" {
        (None, false, None)
    } else if rest.starts_with('=') {
        let expr = rest[1..].trim().trim_end_matches(';').to_string();
        (None, true, Some(expr))
    } else {
        // Handle both RustS+ syntax (just type) and hybrid syntax (-> type)
        let mut type_rest = rest;
        if type_rest.starts_with("->") {
            type_rest = type_rest[2..].trim();
        }
        
        // CRITICAL FIX: Find end of return type, but ignore `=` inside brackets
        // Associated type syntax like `Future[Output = Result[T, E]]` has `=` inside `[]`
        // Only consider `=` at TOP LEVEL (bracket depth 0) as single-line expression marker
        let type_end = find_type_end(type_rest);
        let raw_ret_type = type_rest[..type_end].trim().to_string();
        
        // L-05 CRITICAL FIX: Strip effects clause from return type
        // RustS+: `fn f(x T) effects(write x) U` 
        // Rust:   `fn f(x: T) -> U`
        // The effects(...) clause must NOT appear in Rust output!
        let ret_type = strip_effects_clause(&raw_ret_type);
        
        // If after stripping effects the return type is empty or "()", it's a unit function
        // e.g., `fn log(msg String) effects(io) {` has no return type after stripping
        // In Rust, `fn foo()` is same as `fn foo() -> ()`, so we omit explicit "()"
        let final_return_type = if ret_type.is_empty() || ret_type == "()" {
            None
        } else {
            Some(ret_type)
        };
        
        let after_type = type_rest[type_end..].trim();
        
        if after_type.starts_with('=') {
            let expr = after_type[1..].trim().trim_end_matches(';').to_string();
            (final_return_type, true, Some(expr))
        } else {
            (final_return_type, false, None)
        }
    };
    
    Ok(FunctionSignature {
        name, generics, parameters, return_type, is_pub, is_single_line, single_line_expr,
        write_params,
    })
}

/// Find the end of a return type string, ignoring `=` inside brackets
/// 
/// This is CRITICAL for handling associated type syntax like:
/// `Pin[Box[dyn Future[Output = Result[T, E]] + Send]]`
/// 
/// The `=` in `Output = Result[...]` is NOT a single-line function marker!
/// Only `=` at bracket depth 0 indicates a single-line function body.
/// 
/// Returns the position where the type ends (at `{` or top-level `=`)
fn find_type_end(s: &str) -> usize {
    let mut depth: usize = 0;
    let mut in_string = false;
    let mut prev_char = ' ';
    
    for (i, c) in s.chars().enumerate() {
        // Handle string literals
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
            prev_char = c;
            continue;
        }
        
        if in_string {
            prev_char = c;
            continue;
        }
        
        match c {
            '[' | '(' | '<' => depth += 1,
            ']' | ')' | '>' => depth = depth.saturating_sub(1),
            '{' => {
                // `{` always ends the type (start of function body)
                return i;
            }
            '=' => {
                // CRITICAL: Only consider `=` at top level (depth 0)
                // `=` inside brackets is associated type syntax, NOT single-line fn marker
                if depth == 0 {
                    // Also check it's not `==`, `!=`, `<=`, `>=`, `=>`
                    let next_char = s.chars().nth(i + 1).unwrap_or(' ');
                    if prev_char != '!' && prev_char != '<' && prev_char != '>' 
                       && prev_char != '=' && next_char != '=' && next_char != '>' {
                        return i;
                    }
                }
            }
            _ => {}
        }
        prev_char = c;
    }
    
    s.len()
}

fn parse_parameters(params_str: &str) -> Result<Vec<Parameter>, String> {
    let trimmed = params_str.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    
    let mut parameters = Vec::new();
    let parts = split_by_comma(trimmed);
    
    for part in parts {
        let part = part.trim();
        if part.is_empty() { continue; }
        parameters.push(parse_single_param(part)?);
    }
    
    Ok(parameters)
}

fn split_by_comma(s: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut depth = 0;
    
    for c in s.chars() {
        match c {
            '<' | '[' | '(' => { depth += 1; current.push(c); }
            '>' | ']' | ')' => { depth -= 1; current.push(c); }
            ',' if depth == 0 => {
                result.push(current.trim().to_string());
                current = String::new();
            }
            _ => current.push(c),
        }
    }
    
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

fn parse_single_param(param: &str) -> Result<Parameter, String> {
    let param = param.trim();
    
    // CRITICAL FIX: Handle special self parameters
    // &self, &mut self, self are valid Rust patterns without explicit type annotation
    // RustS+ should accept these directly
    if param == "&self" {
        return Ok(Parameter {
            name: "self".to_string(),
            param_type: "&".to_string(),
            is_borrow: true,
            is_mut_borrow: false,
            is_mut_param: false,
        });
    }
    if param == "&mut self" {
        return Ok(Parameter {
            name: "self".to_string(),
            param_type: "&mut".to_string(),
            is_borrow: true,
            is_mut_borrow: true,
            is_mut_param: false,
        });
    }
    if param == "self" {
        return Ok(Parameter {
            name: "self".to_string(),
            param_type: "".to_string(),
            is_borrow: false,
            is_mut_borrow: false,
            is_mut_param: false,
        });
    }
    // CRITICAL FIX: Handle `mut self` (owned mutable self)
    if param == "mut self" {
        return Ok(Parameter {
            name: "self".to_string(),
            param_type: "".to_string(),
            is_borrow: false,
            is_mut_borrow: false,
            is_mut_param: true,  // Mark as mutable parameter
        });
    }
    // Also handle: self: Type (explicit self type)
    if param.starts_with("self:") {
        let type_str = param[5..].trim().to_string();
        return Ok(Parameter {
            name: "self".to_string(),
            param_type: type_str,
            is_borrow: false,
            is_mut_borrow: false,
            is_mut_param: false,
        });
    }
    
    // CRITICAL FIX: Handle explicit `mut` modifier on parameters
    // RustS+ syntax: `mut get_balance F1` means parameter `get_balance` of type `F1` is mutable
    let (is_mut_param, param_to_parse) = if param.starts_with("mut ") {
        (true, param[4..].trim())
    } else {
        (false, param)
    };
    
    let first_space = param_to_parse.find(' ').ok_or_else(|| format!(
        "Parameter '{}' has no type annotation. All parameters must have explicit types in RustS+.",
        param_to_parse
    ))?;
    
    let name = param_to_parse[..first_space].trim().to_string();
    let type_str = param_to_parse[first_space..].trim().to_string();
    
    if name.is_empty() {
        return Err("Parameter name cannot be empty".to_string());
    }
    
    if type_str.is_empty() {
        return Err(format!(
            "Parameter '{}' has no type annotation. All parameters must have explicit types in RustS+.",
            name
        ));
    }
    
    let (is_borrow, is_mut_borrow) = if type_str.starts_with("&mut ") {
        (true, true)
    } else if type_str.starts_with('&') {
        (true, false)
    } else {
        (false, false)
    };
    
    Ok(Parameter { name, param_type: type_str, is_borrow, is_mut_borrow, is_mut_param })
}

/// Convert a RustS+ function signature to Rust syntax
/// 
/// `has_where_clause`: If true, DON'T add `{` at the end because `where` clause follows
pub fn signature_to_rust(sig: &FunctionSignature) -> String {
    signature_to_rust_impl(sig, false)
}

/// Convert a RustS+ function signature to Rust syntax, controlling brace output
/// 
/// `has_where_clause`: If true, DON'T add `{` at the end because `where` clause follows
pub fn signature_to_rust_with_where(sig: &FunctionSignature, has_where_clause: bool) -> String {
    signature_to_rust_impl(sig, has_where_clause)
}

fn signature_to_rust_impl(sig: &FunctionSignature, has_where_clause: bool) -> String {
    let mut result = String::new();
    
    if sig.is_pub { result.push_str("pub "); }
    
    result.push_str("fn ");
    result.push_str(&sig.name);
    
    if let Some(ref gen) = sig.generics {
        result.push('<');
        result.push_str(gen);
        result.push('>');
    }
    
    result.push('(');
    let params: Vec<String> = sig.parameters.iter()
        .map(|p| {
            // Check if this param needs `mut` due to write effect
            let needs_mut = sig.write_params.contains(&p.name);
            
            // CRITICAL FIX: Handle `self` parameter specially
            // RustS+: `self &` or `self &mut` 
            // Rust:   `&self` or `&mut self`
            if p.name == "self" {
                let type_trimmed = p.param_type.trim();
                if type_trimmed == "&mut" || type_trimmed.starts_with("&mut") {
                    return "&mut self".to_string();
                } else if type_trimmed == "&" || type_trimmed.starts_with("&") {
                    // CRITICAL: If write(self) is declared, use &mut self
                    if needs_mut {
                        return "&mut self".to_string();
                    }
                    return "&self".to_string();
                } else if type_trimmed.is_empty() {
                    // CRITICAL: If write(self) is declared for owned self, use mut self
                    if needs_mut {
                        return "mut self".to_string();
                    }
                    return "self".to_string();
                } else {
                    // Custom self type like `self: Box<Self>`
                    return format!("self: {}", type_trimmed);
                }
            }
            
            // L-06: Transform bare slice type [T] to &[T] for parameters
            // Bare [T] is unsized and cannot be a function parameter in Rust
            let transformed_type = transform_param_type(&p.param_type);
            
            // CRITICAL: Add `mut` if this param has write effect OR explicit mut modifier
            if needs_mut || p.is_mut_param {
                format!("mut {}: {}", p.name, transformed_type)
            } else {
                format!("{}: {}", p.name, transformed_type)
            }
        })
        .collect();
    result.push_str(&params.join(", "));
    result.push(')');
    
    if let Some(ref ret) = sig.return_type {
        // CRITICAL FIX: Don't add arrow if return type already has it
        // Also transform generic brackets: Vec[T] → Vec<T>
        let ret_transformed = transform_generic_brackets(ret);
        let ret_trimmed = ret_transformed.trim();
        if !ret_trimmed.is_empty() {
            if ret_trimmed.starts_with("->") {
                result.push(' ');
                result.push_str(ret_trimmed);
            } else {
                result.push_str(" -> ");
                result.push_str(ret_trimmed);
            }
        }
    }
    
    // CRITICAL FIX: Don't add `{` if there's a `where` clause following
    // The `{` will come after the `where` clause
    if sig.is_single_line {
        if let Some(ref expr) = sig.single_line_expr {
            let mut ctx = CurrentFunctionContext::new();
            ctx.enter(sig, 0);
            let transformed_expr = transform_string_concat(expr, &ctx);
            result.push_str(" { ");
            result.push_str(&transformed_expr);
            result.push_str(" }");
        }
    } else if !has_where_clause {
        // Only add `{` if there's NO where clause following
        result.push_str(" {");
    }
    
    result
}

// Legacy compatibility
#[derive(Debug, Clone)]
pub struct FunctionContext {
    pub name: String,
    pub params: HashMap<String, ParamInfo>,
}

#[derive(Debug, Clone)]
pub struct ParamInfo {
    pub param_type: String,
    pub is_mutable: bool,
}

impl FunctionContext {
    pub fn from_signature(sig: &FunctionSignature) -> Self {
        let mut params = HashMap::new();
        for p in &sig.parameters {
            params.insert(p.name.clone(), ParamInfo {
                param_type: p.param_type.clone(),
                is_mutable: p.is_mut_borrow,
            });
        }
        FunctionContext { name: sig.name.clone(), params }
    }
    
    pub fn is_param(&self, name: &str) -> bool {
        self.params.contains_key(name)
    }
    
    pub fn is_param_mutable(&self, name: &str) -> bool {
        self.params.get(name).map(|p| p.is_mutable).unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_simple_function() {
        let line = "fn add(a i32, b i32) i32 {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert_eq!(sig.name, "add");
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn add(a: i32, b: i32) -> i32 {"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_single_line_function() {
        let line = "fn add(a i32, b i32) i32 = a + b";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert!(sig.is_single_line);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn add(a: i32, b: i32) -> i32 { a + b }"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_borrow_param() {
        let line = "fn read(x &String) {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert!(sig.parameters[0].is_borrow);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn read(x: &String) {"));
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_rust_passthrough() {
        let line = "fn add(a: i32, b: i32) -> i32 {";
        match parse_function_line(line) {
            FunctionParseResult::RustPassthrough => {}
            _ => panic!("Expected RustPassthrough"),
        }
    }
    
    #[test]
    fn test_string_concat_transform() {
        let mut ctx = CurrentFunctionContext::new();
        ctx.params.insert("a".to_string(), "&String".to_string());
        ctx.params.insert("b".to_string(), "&String".to_string());
        
        let expr = r#"a + ", " + b"#;
        let result = transform_string_concat(expr, &ctx);
        assert!(result.contains("a.to_owned()"), "Got: {}", result);
    }
    
    #[test]
    fn test_argument_coercion() {
        assert_eq!(coerce_argument(r#""hello""#, "&String", None), r#"&String::from("hello")"#);
        assert_eq!(coerce_argument(r#"&"hello""#, "&String", None), r#"&String::from("hello")"#);
    }
    
    #[test]
    fn test_tail_return() {
        let mut ctx = CurrentFunctionContext::new();
        ctx.return_type = Some("String".to_string());
        
        assert!(should_be_tail_return("a + b", &ctx, true));
        assert!(should_be_tail_return("a == b", &ctx, true));
        assert!(!should_be_tail_return("println!(\"hi\")", &ctx, true));
        assert!(!should_be_tail_return("a + b;", &ctx, true));
        assert!(!should_be_tail_return("arr[i] = 1", &ctx, true));
        assert!(!should_be_tail_return("self.field = value", &ctx, true));
    }
    
    //=========================================================================
    // L-05: EFFECT ANNOTATION STRIPPING TESTS
    // Effect annotations must NOT appear in Rust output
    //=========================================================================
    
    #[test]
    fn test_strip_effects_clause_basic() {
        // effects(write x) Wallet → Wallet
        assert_eq!(strip_effects_clause("effects(write x) Wallet"), "Wallet");
    }
    
    #[test]
    fn test_strip_effects_clause_io() {
        // effects(io) () → ()
        assert_eq!(strip_effects_clause("effects(io) ()"), "()");
    }
    
    #[test]
    fn test_strip_effects_clause_multiple() {
        // effects(read a, write b) Result<T, E> → Result<T, E>
        assert_eq!(strip_effects_clause("effects(read a, write b) Result<T, E>"), "Result<T, E>");
    }
    
    #[test]
    fn test_strip_effects_clause_no_effects() {
        // String → String (unchanged)
        assert_eq!(strip_effects_clause("String"), "String");
    }
    
    #[test]
    fn test_strip_effects_clause_nested_parens() {
        // effects(write x) Option<(A, B)> → Option<(A, B)>
        assert_eq!(strip_effects_clause("effects(write x) Option<(A, B)>"), "Option<(A, B)>");
    }
    
    #[test]
    fn test_l05_function_with_effects_stripped() {
        // CRITICAL: fn f(x T) effects(write x) U { } must become fn f(x: T) -> U { }
        let line = "fn apply_tx(w Wallet, tx Tx) effects(write w) Wallet {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                // MUST have return type Wallet
                assert!(rust.contains("-> Wallet"), 
                    "L-05: Return type must be 'Wallet', got: {}", rust);
                // MUST NOT have effects clause
                assert!(!rust.contains("effects("), 
                    "L-05: effects clause must NOT appear in Rust output: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_l05_function_with_io_effect() {
        let line = "fn log(msg String) effects(io) {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                // Should have no return type (unit)
                assert!(!rust.contains("->"), 
                    "L-05: No return type for unit function, got: {}", rust);
                // MUST NOT have effects clause  
                assert!(!rust.contains("effects("),
                    "L-05: effects clause must NOT appear in Rust output: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_l05_single_line_with_effects() {
        let line = "fn increment(x i32) effects(pure) i32 = x + 1";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                // Return type must be i32
                assert!(rust.contains("-> i32"),
                    "L-05: Return type must be 'i32', got: {}", rust);
                // MUST NOT have effects clause
                assert!(!rust.contains("effects("),
                    "L-05: effects clause must NOT appear: {}", rust);
                // Body must still work
                assert!(rust.contains("x + 1"),
                    "L-05: Expression body must be preserved: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    /// L-05 CRITICAL: Rust-style params with effects must NOT be classified as passthrough
    #[test]
    fn test_l05_rust_params_with_effects_not_passthrough() {
        // This function has Rust-style params (colon syntax) BUT has RustS+ effects
        // It must NOT be classified as RustPassthrough!
        let line = "fn log(msg: String) effects(io) {";
        match parse_function_line(line) {
            FunctionParseResult::RustPassthrough => {
                panic!("L-05: Function with effects must NOT be RustPassthrough even with Rust-style params");
            }
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                // MUST NOT have effects clause in output
                assert!(!rust.contains("effects("),
                    "L-05: effects clause must NOT appear: {}", rust);
            }
            _ => {} // Other results are also acceptable as long as it's not passthrough
        }
    }
    
    /// L-05: is_rust_syntax must return false for lines with effects
    #[test]
    fn test_is_rust_syntax_with_effects() {
        // This function has Rust-style params but RustS+ effects
        let line = "fn foo(a: i32, b: i32) effects(io) {";
        // is_rust_syntax should return false because of effects
        // (We can't directly test is_rust_syntax since it's private, 
        //  but we can verify the behavior through parse_function_line)
        match parse_function_line(line) {
            FunctionParseResult::RustPassthrough => {
                panic!("L-05: Line with effects() should NOT be treated as Rust passthrough");
            }
            _ => {} // Any other result is fine
        }
    }
    
    //=========================================================================
    // WRITE EFFECT TESTS
    // Parameters with write(param) effect must get `mut` in Rust output
    //=========================================================================
    
    #[test]
    fn test_extract_write_params_basic() {
        // effects(write acc) -> ["acc"]
        let result = extract_write_params("effects(write acc) Account");
        assert_eq!(result, vec!["acc"]);
    }
    
    #[test]
    fn test_extract_write_params_parenthesized() {
        // effects(write(acc)) -> ["acc"]
        let result = extract_write_params("effects(write(acc)) Account");
        assert_eq!(result, vec!["acc"]);
    }
    
    #[test]
    fn test_extract_write_params_multiple() {
        // effects(write from, write to) -> ["from", "to"]
        let result = extract_write_params("effects(write from, write to, io) Account");
        assert_eq!(result, vec!["from", "to"]);
    }
    
    #[test]
    fn test_extract_write_params_no_write() {
        // effects(io, alloc) -> []
        let result = extract_write_params("effects(io, alloc) Account");
        assert!(result.is_empty());
    }
    
    #[test]
    fn test_extract_write_params_self() {
        // effects(write self) -> ["self"]
        let result = extract_write_params("effects(write self) {");
        assert_eq!(result, vec!["self"]);
    }
    
    #[test]
    fn test_write_effect_adds_mut() {
        // fn deposit(acc Account) effects(write acc) Account { 
        // -> fn deposit(mut acc: Account) -> Account {
        let line = "fn deposit(acc Account) effects(write acc) Account {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert_eq!(sig.write_params, vec!["acc"]);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("mut acc: Account"),
                    "Write effect must add mut to param, got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_write_effect_multiple_params() {
        // fn transfer(from Account, to Account, amount i64) effects(write from, write to) {
        let line = "fn transfer(from Account, to Account, amount i64) effects(write from, write to) {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert_eq!(sig.write_params.len(), 2);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("mut from: Account"),
                    "Write effect must add mut to from, got: {}", rust);
                assert!(rust.contains("mut to: Account"),
                    "Write effect must add mut to to, got: {}", rust);
                assert!(rust.contains("amount: i64"),
                    "Non-write param should not have mut, got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_write_effect_self() {
        // fn add_funds(self, amount i64) effects(write self) { 
        // -> fn add_funds(mut self, amount: i64) {
        let line = "fn add_funds(self, amount i64) effects(write self) {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert!(sig.write_params.contains(&"self".to_string()));
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("mut self"),
                    "Write effect on self must produce 'mut self', got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_write_effect_parenthesized_form() {
        // effects(write(acc)) - parenthesized form
        let line = "fn deposit(acc Account) effects(write(acc)) Account {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                assert_eq!(sig.write_params, vec!["acc"]);
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("mut acc: Account"),
                    "Parenthesized write effect must add mut, got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    //=========================================================================
    // FUNCTION POINTER TYPE TESTS
    // fn(T) R must become fn(T) -> R
    //=========================================================================
    
    #[test]
    fn test_transform_fn_pointer_basic() {
        // fn(Account) Account -> fn(Account) -> Account
        let result = transform_fn_pointer_type("fn(Account) Account");
        assert_eq!(result, "fn(Account) -> Account");
    }
    
    #[test]
    fn test_transform_fn_pointer_multiple_params() {
        // fn(A, B) C -> fn(A, B) -> C
        let result = transform_fn_pointer_type("fn(A, B) C");
        assert_eq!(result, "fn(A, B) -> C");
    }
    
    #[test]
    fn test_transform_fn_pointer_already_arrow() {
        // fn(A) -> B (already correct) -> unchanged
        let result = transform_fn_pointer_type("fn(A) -> B");
        assert_eq!(result, "fn(A) -> B");
    }
    
    #[test]
    fn test_transform_fn_pointer_no_return() {
        // fn(A) (no return type) -> unchanged
        let result = transform_fn_pointer_type("fn(A)");
        assert_eq!(result, "fn(A)");
    }
    
    #[test]
    fn test_fn_pointer_in_param() {
        // fn map_accounts(accounts Vec[Account], f fn(Account) Account) Vec[Account]
        let line = "fn map_accounts(accounts Vec[Account], f fn(Account) Account) Vec[Account] {";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                assert!(rust.contains("fn(Account) -> Account"),
                    "Function pointer type must have arrow, got: {}", rust);
                assert!(rust.contains("Vec<Account>"),
                    "Generics must be transformed, got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    // =========================================================================
    // ASSOCIATED TYPE TESTS - Output = Result[T, E] must be preserved
    // =========================================================================
    
    #[test]
    fn test_find_type_end_simple() {
        // Simple type without brackets
        assert_eq!(find_type_end("String {"), 7);
        assert_eq!(find_type_end("i32 ="), 4);
    }
    
    #[test]
    fn test_find_type_end_with_associated_type() {
        // CRITICAL: `=` inside brackets must NOT be treated as end of type
        let input = "Pin[Box[dyn Future[Output = Result[T, E]] + Send]]";
        assert_eq!(find_type_end(input), input.len()); // No `{` or top-level `=`
        
        let input2 = "Pin[Box[dyn Future[Output = Result[T, E]]]] {";
        assert_eq!(find_type_end(input2), input2.len() - 2); // At `{`
    }
    
    #[test]
    fn test_async_trait_method_signature() {
        // Async trait method with associated type Output = Result[...]
        let line = "fn post_blob(&self, data &[u8]) Pin[Box[dyn Future[Output = Result[BlobRef, DAError]] + Send + '_]]";
        match parse_function_line(line) {
            FunctionParseResult::RustSPlusSignature(sig) => {
                let rust = signature_to_rust(&sig);
                // CRITICAL: Must NOT have `{` in the output (no body)
                // CRITICAL: Must preserve `Output = Result`, NOT `Output { Result`
                assert!(rust.contains("Output = Result"),
                    "Associated type = must be preserved, got: {}", rust);
                assert!(!rust.contains("Output {"),
                    "Must NOT have Output {{ , got: {}", rust);
                assert!(rust.contains("Future<"),
                    "Future must be transformed to angle brackets, got: {}", rust);
            }
            _ => panic!("Expected RustSPlusSignature"),
        }
    }
    
    #[test]
    fn test_future_generic_transformation() {
        // Verify Future is in GENERIC_TYPES and gets transformed
        let result = transform_generic_brackets("Future[Output = Result[T, E]]");
        assert_eq!(result, "Future<Output = Result<T, E>>");
        
        let result2 = transform_generic_brackets("Pin[Box[dyn Future[Output = Result[T, E]]]]");
        assert_eq!(result2, "Pin<Box<dyn Future<Output = Result<T, E>>>>");
    }
}
