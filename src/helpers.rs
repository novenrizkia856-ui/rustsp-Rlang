//! Helper utility functions for RustS+ transpiler
//! 
//! Contains general-purpose utility functions used throughout the transpiler:
//! - Comment stripping
//! - Generic bracket transformation  
//! - Semicolon detection
//! - Block/function definition detection
//! - Macro call transformation
//! - Identifier validation

/// Strip inline comments from a line, preserving string literals
pub fn strip_inline_comment(line: &str) -> String {
    let mut result = String::new();
    let mut in_string = false;
    let mut prev_char = ' ';
    let chars: Vec<char> = line.chars().collect();
    
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        
        if c == '"' && prev_char != '\\' {
            in_string = !in_string;
        }
        
        if !in_string && c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            break;
        }
        
        result.push(c);
        prev_char = c;
        i += 1;
    }
    
    result.trim_end().to_string()
}

/// Transform RustS+ generic syntax to Rust generic syntax
/// RustS+ uses square brackets for generics: `Vec[String]`, `HashMap[K, V]`
/// Rust uses angle brackets: `Vec<String>`, `HashMap<K, V>`
pub fn transform_generic_brackets(type_str: &str) -> String {
    let trimmed = type_str.trim();
    
    const GENERIC_TYPES: &[&str] = &[
        // Collections
        "Vec", "HashMap", "HashSet", "BTreeMap", "BTreeSet",
        "VecDeque", "LinkedList", "BinaryHeap",
        // Smart pointers & wrappers
        "Option", "Result", "Box", "Rc", "Arc", "RefCell", "Cell",
        "Mutex", "RwLock", "Cow", "PhantomData", "Weak", "Pin",
        // Conversion traits
        "Into", "From", "TryInto", "TryFrom", "AsRef", "AsMut",
        // Iterator traits
        "Iterator", "IntoIterator", "ExactSizeIterator", "DoubleEndedIterator",
        // Function traits
        "Fn", "FnMut", "FnOnce", "FnPtr",
        // Deref/Borrow traits
        "Deref", "DerefMut", "Borrow", "BorrowMut",
        // Range types
        "Range", "RangeInclusive", "RangeFrom", "RangeTo", "RangeFull",
        // Other common generics
        "Sender", "Receiver", "SyncSender",
        "MaybeUninit", "ManuallyDrop",
        // Serde traits
        "Serialize", "Deserialize",
        // Common external crates
        "Lazy", "OnceCell", "OnceLock",
        // Chrono date/time types
        "DateTime", "NaiveDateTime", "NaiveDate", "NaiveTime",
        "Date", "Local", "FixedOffset",
        // Tokio/async types
        "JoinHandle", "UnboundedReceiver", "UnboundedSender",
        // Common Result/Error wrappers
        "anyhow", "Error",
        // Parking lot types
        "RwLockReadGuard", "RwLockWriteGuard", "MutexGuard",
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
            
            // Find the FIRST occurrence (we'll loop to get all)
            if let Some(pos) = result.find(&pattern) {
                let is_word_boundary = pos == 0 || {
                    let prev_char = result.chars().nth(pos - 1).unwrap_or(' ');
                    !prev_char.is_alphanumeric() && prev_char != '_'
                };
                
                if is_word_boundary {
                    let bracket_start = pos + generic_type.len();
                    if let Some(bracket_end) = find_matching_bracket(&result[bracket_start..]) {
                        let inner = &result[bracket_start + 1..bracket_start + bracket_end];
                        // Recursively transform inner content
                        let transformed_inner = transform_generic_brackets(inner);
                        
                        let before = &result[..pos];
                        let after = &result[bracket_start + bracket_end + 1..];
                        
                        result = format!("{}{}<{}>{}", before, generic_type, transformed_inner, after);
                        changed = true; // Mark that we made a change, loop again
                        break; // Restart the loop to handle nested or subsequent generics
                    }
                }
            }
        }
    }
    
    result
}

/// Find the position of the matching closing bracket for a string starting with `[`
pub fn find_matching_bracket(s: &str) -> Option<usize> {
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

/// Check if a line ends with a binary continuation operator
/// CRITICAL: Also includes `=` for multiline assignments like:
///   let x =
///       some_long_expression()
pub fn ends_with_continuation_operator(line: &str) -> bool {
    let trimmed = line.trim();
    
    if trimmed.ends_with('(') || trimmed.ends_with('[') {
        return true;
    }
    
    // CRITICAL FIX: Handle `=` as continuation operator for multiline assignments
    // But NOT `==`, `!=`, `<=`, `>=`, `=>`
    if trimmed.ends_with('=') {
        let len = trimmed.len();
        if len >= 2 {
            let prev_char = trimmed.chars().nth(len - 2).unwrap_or(' ');
            // Check it's not part of ==, !=, <=, >=, =>
            if prev_char != '=' && prev_char != '!' && prev_char != '<' && prev_char != '>' {
                return true;
            }
        } else {
            // Just `=` alone
            return true;
        }
    }
    
    trimmed.ends_with('^') || trimmed.ends_with('|') || trimmed.ends_with('&')
        || trimmed.ends_with('+') || trimmed.ends_with("+ ")
        || trimmed.ends_with('-') || trimmed.ends_with("- ")
        || trimmed.ends_with('*') || trimmed.ends_with("* ")
        || trimmed.ends_with('/') || trimmed.ends_with("/ ")
        || trimmed.ends_with('%')
        || trimmed.ends_with("||") || trimmed.ends_with("&&")
        || trimmed.ends_with("<<") || trimmed.ends_with(">>")
}


/// Check if a line needs a semicolon (ONLY for non-literal mode)
pub fn needs_semicolon(trimmed: &str) -> bool {
    if trimmed.is_empty() { return false; }
    if trimmed.ends_with(';') { return false; }
    
    if trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
        if trimmed.ends_with('{') && !trimmed.contains('}') {
            return false;
        }
        return true;
    }
    
    if trimmed.ends_with('{') || trimmed.ends_with('}') { return false; }
    if trimmed.ends_with(',') { return false; }
    
    if trimmed.ends_with('^') || trimmed.ends_with('|') || trimmed.ends_with('&')
       || trimmed.ends_with('+') || trimmed.ends_with('-') || trimmed.ends_with('*')
       || trimmed.ends_with('/') || trimmed.ends_with('%')
       || trimmed.ends_with("||") || trimmed.ends_with("&&")
       || trimmed.ends_with("<<") || trimmed.ends_with(">>") {
        return false;
    }
    
    if trimmed.ends_with('(') || trimmed.ends_with('[') {
        return false;
    }
    
    if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") 
       || trimmed.starts_with("struct ") || trimmed.starts_with("enum ")
       || trimmed.starts_with("impl ") || trimmed.starts_with("trait ")
       || trimmed.starts_with("mod ") {
        return false;
    }
    
    // CRITICAL FIX: `where` clause should not get semicolon
    if trimmed == "where" || trimmed.starts_with("where ") {
        return false;
    }
    
    if trimmed.starts_with("if ") || trimmed.starts_with("else") 
       || trimmed.starts_with("for ") || trimmed.starts_with("while ")
       || trimmed.starts_with("loop") || trimmed.starts_with("match ") {
        return false;
    }
    
    if trimmed.starts_with("//") || trimmed.starts_with("/*") {
        return false;
    }
    
    if trimmed.starts_with('*') {
        let after_star = &trimmed[1..];
        if after_star.is_empty() {
            return false;
        }
        let first_char = after_star.chars().next().unwrap();
        if first_char == ' ' || first_char == '*' || first_char == '/' {
            return false;
        }
    }
    
    if trimmed.starts_with('#') { return false; }
    if trimmed == ");" { return false; }
    
    true
}

/// Check if a line is a function definition
pub fn is_function_definition(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("fn ") 
        || trimmed.starts_with("pub fn ")
        || trimmed.starts_with("async fn ") 
        || trimmed.starts_with("pub async fn ")
        || trimmed.starts_with("const fn ") 
        || trimmed.starts_with("pub const fn ")
        || trimmed.starts_with("unsafe fn ") 
        || trimmed.starts_with("pub unsafe fn ")
        || trimmed.starts_with("extern fn ")
        || trimmed.starts_with("pub extern fn ")
}

/// Check if a line starts a Rust block that should NOT trigger literal mode
pub fn is_rust_block_start(line: &str) -> bool {
    let trimmed = line.trim();
    is_function_definition(trimmed)
        || trimmed.starts_with("impl ")
        || trimmed.starts_with("impl<")
        || trimmed.starts_with("mod ")
        || trimmed.starts_with("pub mod ")
        || trimmed.starts_with("trait ")
        || trimmed.starts_with("pub trait ")
        || trimmed.starts_with("use ")
        || trimmed.starts_with("pub use ")
}

/// L-08: Transform RustS+ macro calls to Rust macro calls
pub fn transform_macro_calls(line: &str) -> String {
    let trimmed = line.trim();
    
    if is_function_definition(trimmed) {
        return line.to_string();
    }
    
    if trimmed.starts_with("#[") || trimmed.starts_with("#![") {
        return line.to_string();
    }
    
    const MACROS: &[&str] = &[
        "println", "print", "eprintln", "eprint",
        "format", "panic", "todo", "unimplemented",
        "vec", "dbg", "assert", "assert_eq", "assert_ne",
        "debug_assert", "debug_assert_eq", "debug_assert_ne",
        "write", "writeln", "format_args",
        "include_str", "include_bytes", "concat", "stringify",
        "env", "option_env", "line", "column", "file",
        "module_path", "compile_error",
    ];
    
    // CRITICAL: Path-qualified macros like anyhow::bail, anyhow::anyhow
    // These must be transformed to anyhow::bail!, anyhow::anyhow! etc.
    const PATH_MACROS: &[&str] = &[
        "anyhow::bail",
        "anyhow::anyhow",
        "anyhow::ensure",
        "anyhow::format_err",
        "log::info",
        "log::debug",
        "log::warn",
        "log::error",
        "log::trace",
        "tracing::info",
        "tracing::debug",
        "tracing::warn",
        "tracing::error",
        "tracing::trace",
        "tracing::instrument",
        "tracing::info_span",
        "tracing::debug_span",
        "tracing::warn_span",
        "tracing::error_span",
        "tokio::select",
        "tokio::join",
        "tokio::try_join",
        "futures::select",
        "futures::join",
        "serde_json::json",
    ];
    
    let mut result = line.to_string();
    
    // CRITICAL: First handle path-qualified macros (must be done before simple macros)
    // These are unambiguous since they contain ::
    for macro_name in PATH_MACROS {
        let search_pattern = format!("{}(", macro_name);
        let correct_pattern = format!("{}!(", macro_name);
        
        if result.contains(&search_pattern) && !result.contains(&correct_pattern) {
            result = result.replace(&search_pattern, &format!("{}!(", macro_name));
        }
    }
    
    // Then handle simple macros
    for macro_name in MACROS {
        let search_pattern = format!("{}(", macro_name);
        let correct_pattern = format!("{}!(", macro_name);
        
        if result.contains(&search_pattern) && !result.contains(&correct_pattern) {
            let mut new_result = String::new();
            let chars: Vec<char> = result.chars().collect();
            let mut i = 0;
            
            while i < chars.len() {
                let remaining: String = chars[i..].iter().collect();
                
                if remaining.starts_with(&search_pattern) {
                    let is_word_start = i == 0 || (!chars[i-1].is_alphanumeric() && chars[i-1] != '_');
                    let is_method_call = i > 0 && chars[i-1] == '.';
                    
                    if is_word_start && !is_method_call {
                        let before_paren: String = chars[i..i+macro_name.len()].iter().collect();
                        if before_paren == *macro_name {
                            new_result.push_str(macro_name);
                            new_result.push('!');
                            i += macro_name.len();
                            continue;
                        }
                    }
                }
                
                new_result.push(chars[i]);
                i += 1;
            }
            
            result = new_result;
        }
    }
    
    result
}

/// Transform bare slice types [T] to Vec<T> in struct field definitions
pub fn transform_struct_field_slice_to_vec(line: &str) -> String {
    if !line.contains(':') {
        return line.to_string();
    }
    
    let trimmed = line.trim();
    if let Some(colon_pos) = trimmed.find(':') {
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        let field_name = trimmed[..colon_pos].trim();
        let type_part = trimmed[colon_pos + 1..].trim();
        
        if type_part.starts_with('[') && !type_part.starts_with("&[") {
            let mut depth = 0;
            let mut slice_end = None;
            for (i, c) in type_part.char_indices() {
                match c {
                    '[' => depth += 1,
                    ']' => {
                        depth -= 1;
                        if depth == 0 {
                            slice_end = Some(i);
                            break;
                        }
                    }
                    _ => {}
                }
            }
            
            if let Some(end_pos) = slice_end {
                let element_type = &type_part[1..end_pos];
                let rest = &type_part[end_pos + 1..];
                return format!("{}{}: Vec<{}>{}", leading_ws, field_name, element_type, rest);
            }
        }
    }
    
    line.to_string()
}

/// Check if a string is a valid Rust identifier
pub fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_alphabetic() && first != '_' { return false; }
    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// Check if a string is a field access pattern (e.g., `self.field`, `obj.field`)
/// These should NOT get `let` prefix as they are mutations, not declarations.
/// 
/// Examples:
/// - `self.status` → true (field access)
/// - `obj.field` → true (field access)
/// - `x` → false (simple identifier)
/// - `(a, b)` → false (tuple pattern)
pub fn is_field_access(name: &str) -> bool {
    let trimmed = name.trim();
    
    // Must contain `.` but not start with `..` (spread operator)
    if !trimmed.contains('.') || trimmed.starts_with("..") {
        return false;
    }
    
    // Check that there's a valid identifier before the dot
    if let Some(dot_pos) = trimmed.find('.') {
        let before_dot = &trimmed[..dot_pos];
        // The part before dot should be a valid identifier
        is_valid_identifier(before_dot)
    } else {
        false
    }
}

/// Check if a string is a tuple destructuring pattern (e.g., `(a, b)`, `(x, y, z)`)
/// These need `let` prefix for tuple binding.
///
/// Examples:
/// - `(a, b)` → true
/// - `(current, target)` → true  
/// - `x` → false
/// - `self.field` → false
pub fn is_tuple_pattern(name: &str) -> bool {
    let trimmed = name.trim();
    
    // Must start with `(` and end with `)`
    if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
        return false;
    }
    
    // Extract content between parentheses
    let inner = &trimmed[1..trimmed.len()-1];
    
    // Must have at least one comma (tuple has 2+ elements)
    if !inner.contains(',') {
        return false;
    }
    
    // All parts should be valid identifiers or wildcards
    let parts: Vec<&str> = inner.split(',').collect();
    for part in parts {
        let p = part.trim();
        if p.is_empty() {
            return false;
        }
        // Allow _ (wildcard) or valid identifier
        if p != "_" && !is_valid_identifier(p) {
            return false;
        }
    }
    
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_strip_inline_comment() {
        assert_eq!(strip_inline_comment("x = 10 // comment"), "x = 10");
        assert_eq!(strip_inline_comment("x = \"a // b\""), "x = \"a // b\"");
    }
    
    #[test]
    fn test_transform_generic_brackets() {
        assert_eq!(transform_generic_brackets("Vec[String]"), "Vec<String>");
        assert_eq!(transform_generic_brackets("HashMap[String, i32]"), "HashMap<String, i32>");
    }
    
    #[test]
    fn test_needs_semicolon() {
        assert!(needs_semicolon("x = 10"));
        assert!(!needs_semicolon("fn foo() {"));
        assert!(!needs_semicolon("x = 10;"));
    }
    
    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("foo"));
        assert!(is_valid_identifier("_bar"));
        assert!(!is_valid_identifier("123foo"));
    }
    
    #[test]
    fn test_transform_path_macros() {
        // CRITICAL: Path-qualified macros like anyhow::bail must be transformed
        assert_eq!(
            transform_macro_calls("anyhow::bail(\"error message\")"),
            "anyhow::bail!(\"error message\")"
        );
        assert_eq!(
            transform_macro_calls("anyhow::anyhow(\"error: {}\", x)"),
            "anyhow::anyhow!(\"error: {}\", x)"
        );
        assert_eq!(
            transform_macro_calls("log::info(\"message\")"),
            "log::info!(\"message\")"
        );
        // Already has ! should not be double-transformed
        assert_eq!(
            transform_macro_calls("anyhow::bail!(\"already correct\")"),
            "anyhow::bail!(\"already correct\")"
        );
    }
    
    #[test]
    fn test_transform_simple_macros() {
        assert_eq!(
            transform_macro_calls("println(\"hello\")"),
            "println!(\"hello\")"
        );
        assert_eq!(
            transform_macro_calls("vec(1, 2, 3)"),
            "vec!(1, 2, 3)"
        );
    }
}