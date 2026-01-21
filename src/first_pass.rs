//! First pass analysis for RustS+ transpiler
//!
//! This module contains functions for the first pass of transpilation:
//! - Registering struct/enum/function definitions
//! - Tracking clone requirements for array elements
//! - Transitive clone detection

use std::collections::{HashMap, HashSet};

use crate::helpers::strip_inline_comment;
use crate::detection::detect_array_literal_start;
use crate::clone_helpers::{detect_type_from_element, extract_array_var_from_access, is_cloneable_array_access};
use crate::variable::{VariableTracker, parse_rusts_assignment_ext};
use crate::struct_def::{StructRegistry, is_struct_definition, parse_struct_header};
use crate::enum_def::{EnumRegistry, is_enum_definition, parse_enum_header};
use crate::function::{parse_function_line, FunctionParseResult, FunctionRegistry};

/// Result of first pass analysis
pub struct FirstPassResult {
    pub fn_registry: FunctionRegistry,
    pub struct_registry: StructRegistry,
    pub enum_registry: EnumRegistry,
    pub types_need_clone: HashSet<String>,
}

/// Run the first pass analysis over source lines
pub fn run_first_pass(
    lines: &[&str],
    tracker: &mut VariableTracker,
) -> FirstPassResult {
    let mut fn_registry = FunctionRegistry::new();
    let mut struct_registry = StructRegistry::new();
    let mut enum_registry = EnumRegistry::new();
    
    //=========================================================================
    // CLONE INJECTION TRACKING (L-04 Enhancement)
    // Track: array_var → element_type, types that need Clone
    //=========================================================================
    let mut array_element_types: HashMap<String, String> = HashMap::new();
    let mut types_need_clone: HashSet<String> = HashSet::new();
    let mut current_array_var: Option<String> = None;
    
    let mut brace_depth: usize = 0;
    
    // CRITICAL FIX: Track multi-line function signatures in first pass
    let mut first_pass_fn_acc: Option<String> = None;
    
    // First pass: register structs, enums, functions, track assignments
    for (line_num, line) in lines.iter().enumerate() {
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        
        tracker.scan_for_mut_borrows(&clean_line);
        
        // Register struct names
        if is_struct_definition(trimmed) {
            if let Some(name) = parse_struct_header(trimmed) {
                struct_registry.register(&name);
            }
        }
        
        // Register enum names
        if is_enum_definition(trimmed) {
            if let Some(name) = parse_enum_header(trimmed) {
                enum_registry.register(&name);
            }
        }
        
        //=====================================================================
        // CRITICAL FIX: Handle multi-line function signatures
        // Functions like:
        //   fn create_block(
        //       header BlockHeader,
        //       transactions [Transaction]
        //   ) Block { ... }
        // Must be registered for transform_call_args to work
        //=====================================================================
        
        // Continue accumulating multi-line function
        if let Some(ref mut acc) = first_pass_fn_acc {
            acc.push(' ');
            acc.push_str(trimmed);
            
            let paren_opens = acc.matches('(').count();
            let paren_closes = acc.matches(')').count();
            
            // Signature complete when parens balanced and contains `{`
            if paren_opens == paren_closes && acc.contains('{') {
                if let FunctionParseResult::RustSPlusSignature(sig) = parse_function_line(acc) {
                    fn_registry.register(sig);
                }
                first_pass_fn_acc = None;
            }
            continue;
        }
        
        // Register function signatures (single-line or start of multi-line)
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            let paren_opens = trimmed.matches('(').count();
            let paren_closes = trimmed.matches(')').count();
            
            if paren_opens == paren_closes && trimmed.contains('{') {
                // Complete single-line signature
                if let FunctionParseResult::RustSPlusSignature(sig) = parse_function_line(trimmed) {
                    fn_registry.register(sig);
                }
            } else if paren_opens > paren_closes {
                // Start of multi-line signature
                first_pass_fn_acc = Some(trimmed.to_string());
            }
        }
        
        //=====================================================================
        // CLONE TRACKING: Detect array assignments and their element types
        //=====================================================================
        
        // Detect array literal start: `events = [`
        if let Some((var_name, _, _)) = detect_array_literal_start(trimmed) {
            current_array_var = Some(var_name);
        }
        
        // Detect array elements and extract type
        if current_array_var.is_some() && !trimmed.starts_with('[') && !trimmed.is_empty() {
            if trimmed == "]" {
                current_array_var = None;
            } else if let Some(ref var) = current_array_var {
                // Try to detect type from this element
                if let Some(elem_type) = detect_type_from_element(trimmed) {
                    array_element_types.insert(var.clone(), elem_type);
                }
            }
        }
        
        // Detect array access that will get .clone(): `x = arr[i]`
        if let Some((_, _, value, _, _)) = parse_rusts_assignment_ext(&clean_line) {
            if is_cloneable_array_access(&value) {
                if let Some(arr_var) = extract_array_var_from_access(&value) {
                    // Mark the element type as needing Clone
                    if let Some(elem_type) = array_element_types.get(&arr_var) {
                        types_need_clone.insert(elem_type.clone());
                    }
                }
            }
        }
        
        brace_depth += trimmed.matches('{').count();
        brace_depth = brace_depth.saturating_sub(trimmed.matches('}').count());
        
        if trimmed.starts_with("let ") { continue; }
        
        // CRITICAL: Use extended parser to detect explicit `mut` keyword
        // `mut x = 10` means x is DECLARED here, subsequent `x = ...` are mutations
        if let Some((var_name, var_type, value, _is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            tracker.track_assignment(line_num, &var_name, var_type, &value, false);
            // If explicit mut, mark variable as mutable immediately
            if is_explicit_mut {
                tracker.mark_mut_borrowed(&var_name); // Use existing mechanism to ensure mut
            }
        }
    }
    
    //=========================================================================
    // TRANSITIVE CLONE DETECTION
    // If Event needs Clone and contains Node, then Node also needs Clone
    // We scan type definitions to find nested type references
    //=========================================================================
    let type_contents = build_type_contents(lines, &struct_registry, &enum_registry);
    propagate_clone_requirements(&mut types_need_clone, &type_contents);
    
    FirstPassResult {
        fn_registry,
        struct_registry,
        enum_registry,
        types_need_clone,
    }
}

/// Build a map of type → contained types for transitive clone detection
fn build_type_contents(
    lines: &[&str],
    struct_registry: &StructRegistry,
    enum_registry: &EnumRegistry,
) -> HashMap<String, Vec<String>> {
    let mut in_type_def: Option<String> = None;  // Currently inside which type definition
    let mut type_contents: HashMap<String, Vec<String>> = HashMap::new(); // type → contained types
    
    for line in lines.iter() {
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        
        // Detect struct/enum definition start
        if is_struct_definition(trimmed) {
            if let Some(name) = parse_struct_header(trimmed) {
                in_type_def = Some(name);
            }
        } else if is_enum_definition(trimmed) {
            if let Some(name) = parse_enum_header(trimmed) {
                in_type_def = Some(name);
            }
        } else if trimmed == "}" && in_type_def.is_some() {
            in_type_def = None;
        } else if let Some(ref type_name) = in_type_def {
            // We're inside a type definition - look for references to other types
            // Check for patterns like: Init(Node), field Node, Node,
            for struct_name in struct_registry.names.iter() {
                if trimmed.contains(struct_name) {
                    type_contents.entry(type_name.clone())
                        .or_insert_with(Vec::new)
                        .push(struct_name.clone());
                }
            }
            for enum_name in enum_registry.names.iter() {
                if trimmed.contains(enum_name) && enum_name != type_name {
                    type_contents.entry(type_name.clone())
                        .or_insert_with(Vec::new)
                        .push(enum_name.clone());
                }
            }
        }
    }
    
    type_contents
}

/// Propagate Clone requirement transitively
/// Repeat until no new types are added
fn propagate_clone_requirements(
    types_need_clone: &mut HashSet<String>,
    type_contents: &HashMap<String, Vec<String>>,
) {
    loop {
        let mut added_any = false;
        for type_name in types_need_clone.clone().iter() {
            if let Some(contained) = type_contents.get(type_name) {
                for contained_type in contained {
                    if !types_need_clone.contains(contained_type) {
                        types_need_clone.insert(contained_type.clone());
                        added_any = true;
                    }
                }
            }
        }
        if !added_any {
            break;
        }
    }
}