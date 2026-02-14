//! Look-ahead Lowering Utilities
//!
//! Functions that look ahead in the source to make parsing decisions.
//! These are critical for handling multi-line constructs and determining
//! context-dependent behavior.

use crate::helpers::strip_inline_comment;

/// Check if the next non-empty line is a closing brace
/// 
/// Used to determine if current expression is the last one before a block ends.
pub fn check_before_closing_brace(lines: &[&str], line_num: usize) -> bool {
    for future_line in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future_line);
        let ft = ft.trim();
        if !ft.is_empty() {
            return ft == "}" || ft.starts_with("}");
        }
    }
    false
}

/// Check if the next non-empty line starts with `else`
/// 
/// Used for if-expression assignment handling.
pub fn check_next_is_else(lines: &[&str], line_num: usize) -> bool {
    for future_line in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future_line).trim().to_string();
        if ft.is_empty() { 
            continue; 
        }
        return ft.starts_with("else") || ft.starts_with("} else");
    }
    false
}

/// Check if the next non-empty line is a `where` clause
/// 
/// This is CRITICAL for function signature handling:
/// - Functions with `where` clauses have their `{` after the `where` clause
/// - We must NOT add `{` to the function signature line
pub fn check_next_line_is_where(lines: &[&str], line_num: usize) -> bool {
    for future in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future);
        let ft_trim = ft.trim();
        if ft_trim.is_empty() {
            continue;
        }
        return ft_trim.starts_with("where") && 
            (ft_trim == "where" || ft_trim.chars().nth(5).map(|c| c.is_whitespace() || c == '\n').unwrap_or(true));
    }
    false
}

/// Check if the next non-empty line starts with `|` (pipe)
/// 
/// Used for multi-pattern match arm detection:
/// ```text
/// Pattern1 { field1, .. }       // first pattern - next line has |
/// | Pattern2 { field2, .. }     // continuation - next line has |
/// | Pattern3 { field3, .. } { body }  // final - has body
/// ```
pub fn check_next_line_starts_with_pipe(lines: &[&str], line_num: usize) -> bool {
    for future in lines.iter().skip(line_num + 1) {
        let ft = strip_inline_comment(future);
        let ft_trim = ft.trim();
        if ft_trim.is_empty() {
            continue;
        }
        return ft_trim.starts_with('|');
    }
    false
}

/// Check if next line is a method chain continuation (starts with `.`)
pub fn check_next_line_is_method_chain(lines: &[&str], line_num: usize) -> bool {
    for next in lines.iter().skip(line_num + 1) {
        let binding = strip_inline_comment(next);
        let trimmed = binding.trim();
        if trimmed.is_empty() {
            continue;
        }
        return trimmed.starts_with('.');
    }
    false
}

/// Check if next line closes an expression (starts with ), ], etc.)
/// 
/// Used to determine if current line is the last argument in a function call
/// or array literal.
pub fn check_next_line_closes_expr(lines: &[&str], line_num: usize) -> bool {
    lines.get(line_num + 1)
        .map(|next| {
            let binding = strip_inline_comment(next);
            let t = binding.trim();
            t.starts_with(')') 
                || t.starts_with(']') 
                || t.starts_with("})") 
                || t.starts_with(");") 
                || t.starts_with("];")
        })
        .unwrap_or(false)
}

/// Disabled: Detect if match arm body starts with if expression
/// 
/// CRITICAL FIX: This was disabled because the previous logic was BROKEN:
/// - It detected any `if` at start of arm body as "if expression"
/// - This triggered special handling that removed the `{` from pattern
/// - But it DIDN'T add matching `(` for the `)` in closing
/// - Result: unbalanced delimiters like `}),` without `(`
///
/// The normal transformation `Pattern => { ... },` handles ALL cases correctly.
pub fn detect_arm_has_if_expr(_lines: &[&str], _line_num: usize, _start_depth: usize) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_check_before_closing_brace() {
        let lines = vec!["    x", "    }"];
        assert!(check_before_closing_brace(&lines, 0));
        
        let lines = vec!["    x", "    y", "    }"];
        assert!(!check_before_closing_brace(&lines, 0));
        assert!(check_before_closing_brace(&lines, 1));
    }
    
    #[test]
    fn test_check_next_is_else() {
        let lines = vec!["    }", "    else {"];
        assert!(check_next_is_else(&lines, 0));
        
        let lines = vec!["    }", "    something_else"];
        assert!(!check_next_is_else(&lines, 0));
    }
    
    #[test]
    fn test_check_next_line_is_where() {
        let lines = vec!["fn foo<T>(x: T)", "where", "    T: Clone"];
        assert!(check_next_line_is_where(&lines, 0));
        
        let lines = vec!["fn foo<T>(x: T) {"];
        assert!(!check_next_line_is_where(&lines, 0));
    }
}
