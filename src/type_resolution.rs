//! Minimal type resolution pass for lowering-time semantic checks.
//!
//! This pass resolves call-expression return types from registered function signatures,
//! allowing lowering rules to be type-aware without full type inference.

use crate::function::FunctionRegistry;

#[derive(Debug, Clone)]
pub struct TypeResolutionPass {
    fn_registry: FunctionRegistry,
}

impl TypeResolutionPass {
    pub fn new(fn_registry: FunctionRegistry) -> Self {
        Self { fn_registry }
    }

    /// Resolve return type for a call-ish expression.
    /// Supports chains like `foo().bar().baz()` by resolving the root call `foo()`.
    pub fn resolve_return_type(&self, expr: &str) -> Option<String> {
        let root = extract_root_call_name(expr)?;
        self.fn_registry
            .get(&root)
            .and_then(|sig| sig.return_type.clone())
    }

    pub fn return_type_is_result(&self, expr: &str) -> bool {
        self.resolve_return_type(expr)
            .map(|t| is_result_type(&t))
            .unwrap_or(false)
    }
}

pub fn is_result_type(ty: &str) -> bool {
    let t = ty.trim();
    t.starts_with("Result<") || t.starts_with("Result[")
}

pub fn has_explicit_result_handling(expr: &str) -> bool {
    let t = expr.trim();
    t.contains(".expect(") || t.contains(".unwrap(") || t.ends_with('?') || t.contains("?")
}

fn extract_root_call_name(expr: &str) -> Option<String> {
    let t = expr.trim();
    let chars: Vec<char> = t.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if chars[i].is_alphabetic() || chars[i] == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            if i < chars.len() && chars[i] == '(' {
                return Some(chars[start..i].iter().collect());
            }
        }
        i += 1;
    }
    None
}
