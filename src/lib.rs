// Core modules
pub mod variable;
pub mod scope;
pub mod function;
pub mod struct_def;
pub mod enum_def;
pub mod control_flow;
pub mod error_msg;
pub mod semantic_check;
pub mod anti_fail_logic;
pub mod rust_sanity;
pub mod hex_normalizer;

//IR-based modules
pub mod ast;
pub mod hir;
pub mod eir;
pub mod parser;

pub mod source_map;

// Modularized transpiler components
pub mod helpers;
pub mod modes;
pub mod detection;
pub mod transform_literal;
pub mod transform_array;
pub mod clone_helpers;
pub mod postprocess;

// Re-export IR types for convenience
pub use ast::{Span, Spanned, EffectDecl};
pub use hir::{BindingId, BindingInfo, ScopeResolver, HirModule};
pub use eir::{Effect, EffectSet, EffectContext, EffectInference};
pub use parser::{Lexer, FunctionParser, extract_function_signatures};

use std::collections::{HashSet, HashMap};

use variable::{VariableTracker, parse_rusts_assignment, parse_rusts_assignment_ext, expand_value};
use scope::ScopeAnalyzer;
use function::{
    parse_function_line, signature_to_rust, FunctionParseResult,
    FunctionRegistry, CurrentFunctionContext,
    transform_string_concat, transform_call_args, should_be_tail_return
};
use struct_def::{
    StructRegistry, is_struct_definition, parse_struct_header, 
    transform_struct_field,
};
use enum_def::{
    EnumRegistry, EnumParseContext,
    is_enum_definition, parse_enum_header, transform_enum_variant,
};
use control_flow::{
    MatchModeStack, is_match_start, is_match_arm_pattern,
    transform_arm_pattern, transform_arm_close, transform_arm_close_with_parens,
    is_if_assignment, parse_control_flow_assignment,
    is_single_line_arm, transform_single_line_arm,
    transform_enum_struct_init,
    MatchStringContext, transform_match_for_string_patterns, pattern_is_string_literal,
};
use hex_normalizer::normalize_hex_literals;

// Import from modularized components
use helpers::{
    strip_inline_comment, transform_generic_brackets, find_matching_bracket,
    ends_with_continuation_operator, needs_semicolon, is_function_definition,
    is_rust_block_start, transform_macro_calls, transform_struct_field_slice_to_vec,
    is_valid_identifier,
};
use modes::{
    LiteralKind, LiteralModeStack, ArrayModeStack, UseImportMode,
    is_multiline_use_import_start, transform_use_import_item,
};
use detection::{
    detect_struct_literal_start, detect_bare_struct_literal,
    detect_bare_enum_literal, detect_enum_literal_start, detect_array_literal_start,
};
use transform_literal::{
    transform_literal_field, transform_literal_field_with_ctx,
    transform_nested_struct_value, find_field_eq_top_level, find_field_eq,
    is_valid_field_name, is_string_literal, should_clone_field_value,
};
use transform_array::{
    transform_array_element, transform_enum_struct_init_in_array,
    has_assignment_equals_in_braces, transform_enum_fields_inline,
};
use clone_helpers::{
    transform_array_access_clone, is_valid_array_base, extract_arm_pattern,
    detect_type_from_element, extract_array_var_from_access, is_cloneable_array_access,
};
use postprocess::{
    fix_bare_mut_declaration, strip_effects_from_line, strip_outer_keyword,
};


//===========================================================================
// MAIN PARSER
//===========================================================================

pub fn parse_rusts(source: &str) -> String {
    // CRITICAL: Normalize custom hex literals FIRST
    // Convert 0xMERKLE01 → 0x<valid_hex>, 0xWALLET01 → 0x<valid_hex>, etc.
    // This ensures ALL hex literals are valid Rust hex before transpilation
    let normalized_source = normalize_hex_literals(source);
    
    let lines: Vec<&str> = normalized_source.lines().collect();
    let mut tracker = VariableTracker::new();
    
    // Run scope analysis
    let mut scope_analyzer = ScopeAnalyzer::new();
    scope_analyzer.analyze(&normalized_source);
    
    // Build registries
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
    
    // Propagate Clone requirement transitively
    // Repeat until no new types are added
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
    
    let mut output_lines: Vec<String> = Vec::new();
    
    // Reset for second pass
    brace_depth = 0;
    let mut in_function_body = false;
    let mut function_start_brace = 0;
    let mut current_fn_ctx = CurrentFunctionContext::new();
    
    // Struct/enum definition contexts
    let mut in_struct_def = false;
    let mut struct_def_depth = 0;
    let mut enum_ctx = EnumParseContext::new();
    
    // LITERAL MODE - the key to fixing the bug!
    let mut literal_mode = LiteralModeStack::new();
    
    // ARRAY MODE - for multiline array literal expressions
    let mut array_mode = ArrayModeStack::new();
    // Bracket depth tracking for array mode
    let mut bracket_depth: usize = 0;
    
    // MATCH MODE - for RustS+ match syntax transformation
    let mut match_mode = MatchModeStack::new();
    
    // USE IMPORT MODE - for multiline use import blocks
    // Tracks `pub use module::{ Item1 Item2 }` to add commas between items
    let mut use_import_mode = UseImportMode::new();
    
    // IF EXPRESSION ASSIGNMENT MODE - tracks `x = if cond {` for semicolon at end
    let mut if_expr_assignment_depth: Option<usize> = None;
    
    // MULTI-LINE FUNCTION SIGNATURE MODE
    // Accumulates lines when function signature spans multiple lines
    let mut multiline_fn_acc: Option<String> = None;
    let mut multiline_fn_leading_ws: String = String::new();
    
    // EXPRESSION CONTINUATION TRACKING
    // Tracks when previous line ended with a binary operator
    let mut prev_line_was_continuation = false;
    
    // MULTILINE EXPRESSION DEPTH TRACKING
    // Tracks unclosed parens/brackets for multiline function calls, macros, arrays
    // When > 0, we're inside a multiline expression and should NOT add semicolons
    let mut multiline_expr_depth: i32 = 0;
    
    for (line_num, line) in lines.iter().enumerate() {
        // CRITICAL FIX: Strip BOM (Byte Order Mark) from the line
        // BOM is \u{FEFF} and can appear at the start of UTF-8 files
        // It's invisible but causes issues if not handled
        let line = line.trim_start_matches('\u{FEFF}');
        
        let clean_line = strip_inline_comment(line);
        let trimmed = clean_line.trim();
        let leading_ws: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        
        //=======================================================================
        // MULTILINE EXPRESSION DEPTH TRACKING
        // Track unclosed parens/brackets for multiline expressions like:
        //   format!(           <- depth becomes 1
        //       "{}",          <- still depth 1, no semicolon
        //       value          <- still depth 1, no semicolon  
        //   )                  <- depth becomes 0
        // This ensures we don't add semicolons INSIDE multiline expressions
        //=======================================================================
        // Save depth BEFORE processing this line
        let multiline_depth_before = multiline_expr_depth;
        
        // Update depth based on this line's parens/brackets
        {
            let mut in_string = false;
            let mut escape_next = false;
            for c in trimmed.chars() {
                if escape_next {
                    escape_next = false;
                    continue;
                }
                if c == '\\' && in_string {
                    escape_next = true;
                    continue;
                }
                if c == '"' {
                    in_string = !in_string;
                    continue;
                }
                if !in_string {
                    match c {
                        '(' | '[' => multiline_expr_depth += 1,
                        ')' | ']' => multiline_expr_depth -= 1,
                        _ => {}
                    }
                }
            }
            // Ensure depth doesn't go negative (defensive)
            if multiline_expr_depth < 0 {
                multiline_expr_depth = 0;
            }
        }
        
        // CRITICAL FIX: Skip semicolon ONLY if we're STILL inside a multiline expression
        // after processing this line. This ensures CLOSING lines (like `)`) get semicolons!
        // - Lines INSIDE: depth_before > 0 AND depth_after > 0 → skip semicolon
        // - CLOSING line: depth_before > 0 AND depth_after == 0 → ADD semicolon!
        let inside_multiline_expr = multiline_depth_before > 0 && multiline_expr_depth > 0;
        
        // LOOK-AHEAD: Check if next line starts with `.` (method chain continuation)
        // If so, don't add semicolon to current line
        let next_line_is_method_chain = lines.get(line_num + 1)
            .map(|next| {
                let next_trimmed = strip_inline_comment(next).trim().to_string();
                next_trimmed.starts_with('.')
            })
            .unwrap_or(false);
        
        //=======================================================================
        // MULTI-LINE FUNCTION SIGNATURE ACCUMULATION
        // When function signature spans multiple lines like:
        //   fn foo(
        //       a u64,
        //       b u64
        //   ) RetType {
        // We accumulate all lines until we have the complete signature
        //=======================================================================
        if let Some(ref mut acc) = multiline_fn_acc {
            // Continue accumulating
            acc.push(' ');
            acc.push_str(trimmed);
            
            // Check if signature is complete (has matching parens AND ends with {)
            let paren_opens = acc.matches('(').count();
            let paren_closes = acc.matches(')').count();
            
            if paren_opens == paren_closes && acc.ends_with('{') {
                // Signature is complete! Process it
                let complete_sig = acc.clone();
                multiline_fn_acc = None;
                
                // Track function context
                in_function_body = true;
                function_start_brace = brace_depth + 1;
                
                if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(&complete_sig) {
                    current_fn_ctx.enter(sig, function_start_brace);
                }
                
                // Process the complete signature
                match parse_function_line(&complete_sig) {
                    FunctionParseResult::RustSPlusSignature(sig) => {
                        let rust_sig = signature_to_rust(&sig);
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, rust_sig));
                    }
                    FunctionParseResult::RustPassthrough => {
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                    FunctionParseResult::Error(e) => {
                        output_lines.push(format!("{}// COMPILE ERROR: {}", multiline_fn_leading_ws, e));
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                    FunctionParseResult::NotAFunction => {
                        output_lines.push(format!("{}{}", multiline_fn_leading_ws, complete_sig));
                    }
                }
                
                // Update brace depth for the opening brace
                brace_depth += 1;
                continue;
            } else {
                // Not complete yet, continue accumulating
                continue;
            }
        }
        
        // Check if this line starts a multi-line function signature
        if (trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ")) && trimmed.contains('(') {
            let paren_opens = trimmed.matches('(').count();
            let paren_closes = trimmed.matches(')').count();
            
            // If parens are unbalanced, start accumulating
            if paren_opens > paren_closes {
                multiline_fn_acc = Some(trimmed.to_string());
                multiline_fn_leading_ws = leading_ws.clone();
                continue;
            }
        }
        
        // Track function context (for single-line signatures)
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            in_function_body = true;
            function_start_brace = brace_depth + 1;
            
            if let FunctionParseResult::RustSPlusSignature(ref sig) = parse_function_line(trimmed) {
                current_fn_ctx.enter(sig, function_start_brace);
            }
        }
        
        // Calculate brace depth BEFORE processing
        let prev_depth = brace_depth;
        let opens = trimmed.matches('{').count();
        let closes = trimmed.matches('}').count();
        brace_depth += opens;
        brace_depth = brace_depth.saturating_sub(closes);
        
        // Calculate bracket depth for array literals
        let prev_bracket_depth = bracket_depth;
        let bracket_opens = trimmed.matches('[').count();
        let bracket_closes = trimmed.matches(']').count();
        bracket_depth += bracket_opens;
        bracket_depth = bracket_depth.saturating_sub(bracket_closes);
        
        // Exit function context
        if in_function_body && brace_depth < function_start_brace && trimmed == "}" {
            in_function_body = false;
            current_fn_ctx.exit();
        }
        
        // Check for return detection
        let is_before_closing_brace = {
            let mut found = false;
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft = ft.trim();
                if !ft.is_empty() {
                    found = ft == "}" || ft.starts_with("}");
                    break;
                }
            }
            found
        };
        
        if trimmed.is_empty() {
            // Empty line breaks any continuation
            prev_line_was_continuation = false;
            output_lines.push(String::new());
            continue;
        }
        
        //=======================================================================
        // USE IMPORT MODE EXIT CHECK - closing brace of use { } block
        //=======================================================================
        if use_import_mode.is_active() && trimmed == "}" {
            if use_import_mode.should_exit(brace_depth) {
                use_import_mode.exit();
                // Closing brace of use block needs semicolon
                output_lines.push(format!("{}}};", leading_ws));
                continue;
            }
        }
        
        //=======================================================================
        // USE IMPORT MODE ACTIVE - transform items to add commas
        //=======================================================================
        if use_import_mode.is_active() {
            let transformed = transform_use_import_item(&clean_line);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // USE IMPORT MODE START - detect `pub use path::{` or `use path::{`
        //=======================================================================
        if let Some(is_pub) = is_multiline_use_import_start(trimmed) {
            // Enter use import mode - brace_depth is AFTER this line's { is counted
            use_import_mode.enter(brace_depth, is_pub);
            // Output the opening line as-is (no semicolon yet!)
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        //=======================================================================
        // ARRAY MODE EXIT CHECK - must happen BEFORE other processing
        //=======================================================================
        if array_mode.is_active() && trimmed.contains(']') {
            // Check if this closes the array
            if array_mode.should_exit(bracket_depth) {
                if let Some(entry) = array_mode.exit() {
                    // Transform the closing line (may have content before ])
                    let transformed = transform_array_element(&clean_line);
                    
                    // Determine suffix based on context
                    let suffix = if entry.is_assignment { ";" } else { "" };
                    
                    // Output the closing with proper suffix
                    let close_line = if transformed.trim() == "]" {
                        format!("{}]{}", leading_ws, suffix)
                    } else {
                        // Has content before ], like `    Event::Query(1)]`
                        let without_bracket = transformed.trim().trim_end_matches(']').trim_end_matches(',');
                        format!("{}    {},\n{}]{}", leading_ws, without_bracket, leading_ws, suffix)
                    };
                    output_lines.push(close_line);
                    continue;
                }
            }
        }
        
        //=======================================================================
        // ARRAY MODE ACTIVE - transform as element, NO let, NO ;
        //=======================================================================
        if array_mode.is_active() {
            let transformed = transform_array_element(&clean_line);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // LITERAL MODE EXIT CHECK - must happen BEFORE processing
        //=======================================================================
        if literal_mode.is_active() && trimmed == "}" {
            // Check if this closes the literal
            if literal_mode.should_exit(brace_depth) {
                let was_assignment = literal_mode.current_is_assignment();
                literal_mode.exit();
                // Determine suffix:
                // - Nested literal (still in parent literal): `,`
                // - Top-level assignment literal: `;`
                // - Top-level return expression (bare literal): no suffix
                let suffix = if literal_mode.is_active() { 
                    "," 
                } else if was_assignment {
                    ";"
                } else {
                    "" // bare return expression
                };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        //=======================================================================
        // LITERAL MODE ACTIVE - transform as field, NO let, NO ;
        //=======================================================================
        if literal_mode.is_active() {
            // CRITICAL FIX: Pass function context so slice params get .to_vec()
            let transformed = transform_literal_field_with_ctx(&clean_line, Some(&current_fn_ctx));
            
            // Check if this line ALSO starts a nested literal
            // CRITICAL FIX: Use opens > closes to detect multi-line nested literals
            // A line like `address = Address { value = addr_hash },` has opens=closes=1
            // so it's a COMPLETE single-line literal, not a multi-line start.
            // Only enter nested mode if we open MORE braces than we close.
            if trimmed.contains('{') && opens > closes {
                // Enter nested literal mode - nested literals are fields, not assignments
                let kind = if trimmed.contains("::") { 
                    LiteralKind::EnumVariant 
                } else { 
                    LiteralKind::Struct 
                };
                literal_mode.enter(kind, prev_depth + opens, false);
            }
            
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // MATCH MODE - RustS+ match syntax transformation
        //=======================================================================
        
        // Check for match arm body close FIRST (before other checks)
        if match_mode.is_active() && trimmed == "}" {
            if match_mode.should_exit_arm(brace_depth) {
                // L-02: Get parens state BEFORE exiting arm body
                let uses_parens = match_mode.arm_uses_parens();
                match_mode.exit_arm_body();
                // Add comma after arm body close
                // L-02: Close with ), if using parenthesized form
                output_lines.push(transform_arm_close_with_parens(&clean_line, uses_parens));
                continue;
            }
            // Check if this closes the entire match
            if match_mode.should_exit_match(brace_depth) {
                let needs_semi = match_mode.current_is_assignment();
                match_mode.exit_match();
                // Add semicolon if this was an assignment
                let suffix = if needs_semi { ";" } else { "" };
                output_lines.push(format!("{}}}{}", leading_ws, suffix));
                continue;
            }
        }
        
        // Check for match arm pattern (when expecting one)
        if match_mode.expecting_arm_pattern() && is_match_arm_pattern(trimmed) {
            // Check if it's a SINGLE-LINE arm: `pattern { body }`
            if is_single_line_arm(trimmed) {
                // Get return type for string transformation
                let ret_type = current_fn_ctx.return_type.as_deref();
                let transformed = transform_single_line_arm(&clean_line, ret_type);
                output_lines.push(transformed);
                // Stay in expecting_arm_pattern mode (don't enter arm body)
                continue;
            }
            
            //===================================================================
            // L-02: Expression Context for Match Arms with if/else
            // 
            // In Rust, if-else expressions in match arms work directly:
            //   `Pattern => if cond { a } else { b },`
            // No parentheses needed (they cause compiler warnings).
            //
            // We only need to ensure proper comma after the arm body.
            //===================================================================
            let mut arm_has_if_expr = false;
            let mut arm_body_lines = Vec::new();
            let mut temp_depth = prev_depth + opens;
            
            // Look ahead to collect arm body and detect if expression
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft_trim = ft.trim();
                
                // Track depth
                let ft_opens = ft_trim.matches('{').count();
                let ft_closes = ft_trim.matches('}').count();
                
                temp_depth += ft_opens;
                temp_depth = temp_depth.saturating_sub(ft_closes);
                
                // Check if this closes the arm
                if ft_trim == "}" && temp_depth < prev_depth + opens {
                    break;
                }
                
                arm_body_lines.push(ft_trim.to_string());
                
                // Detect if first meaningful line is an if expression
                if arm_body_lines.len() == 1 && !ft_trim.is_empty() {
                    if ft_trim.starts_with("if ") && !ft_trim.contains("let ") {
                        arm_has_if_expr = true;
                    }
                }
            }
            
            // Transform arm pattern - NEVER use parentheses, just => 
            if arm_has_if_expr {
                // L-09: Use plain => for if expression arms (no parens)
                let pattern = extract_arm_pattern(trimmed);
                output_lines.push(format!("{}{} =>", leading_ws, pattern));
            } else {
                // Standard: Use => { for block arms
                let transformed = transform_arm_pattern(&clean_line);
                output_lines.push(transformed);
            }
            
            // Enter arm body
            // L-02: Pass arm_has_if_expr so we know to close with ) instead of }
            // CRITICAL: Use brace_depth (which accounts for both opens AND closes on this line)
            // instead of prev_depth + opens (which only counts opens).
            // This is important for patterns like `Event::Credit { id, amount } {`
            // where opens=2, closes=1, so net depth increase is 1, not 2.
            match_mode.enter_arm_body(brace_depth, arm_has_if_expr);
            continue;
        }
        
        // Check for match expression start
        if is_match_start(trimmed) {
            // Check if it's an assignment: `x = match expr {`
            let is_assignment = parse_control_flow_assignment(trimmed).is_some();
            
            //===================================================================
            // BUG C FIX: Detect if match has string literal patterns
            // If so, transform match expr to add .as_str()
            // Look ahead to find patterns
            //===================================================================
            let mut match_string_ctx = MatchStringContext::from_match_line(trimmed);
            
            // Look ahead to detect string literal patterns
            for future_line in lines.iter().skip(line_num + 1) {
                let ft = strip_inline_comment(future_line);
                let ft_trim = ft.trim();
                
                // Stop at match closing brace at same depth (heuristic)
                if ft_trim == "}" {
                    break;
                }
                
                // Check if this is a pattern line with string literal
                if pattern_is_string_literal(ft_trim) {
                    match_string_ctx.has_string_patterns = true;
                    break; // Found one, no need to continue
                }
                
                // Also check single-line arms with string patterns
                if ft_trim.starts_with('"') && ft_trim.contains('{') {
                    match_string_ctx.has_string_patterns = true;
                    break;
                }
            }
            
            // Determine if we need .as_str() transformation
            let needs_as_str = match_string_ctx.needs_as_str();
            
            if let Some((var_name, match_expr)) = parse_control_flow_assignment(trimmed) {
                // CRITICAL FIX: Check if variable is a function parameter
                let is_param = current_fn_ctx.params.contains_key(&var_name);
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                // Use same logic: add let unless it's mutating a parameter
                let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
                
                // Transform match expression if needed
                let transformed_match_expr = if needs_as_str {
                    transform_match_for_string_patterns(&match_expr, true)
                } else {
                    match_expr
                };
                
                if needs_let {
                    let keyword = if needs_mut { "let mut" } else { "let" };
                    output_lines.push(format!("{}{} {} = {}", leading_ws, keyword, var_name, transformed_match_expr));
                } else {
                    output_lines.push(format!("{}{} = {}", leading_ws, var_name, transformed_match_expr));
                }
            } else {
                // Bare match expression (like return expression)
                let transformed = if needs_as_str {
                    transform_match_for_string_patterns(trimmed, true)
                } else {
                    trimmed.to_string()
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            // Enter match mode with assignment tracking
            match_mode.enter_match(prev_depth, is_assignment);
            continue;
        }
        
        //=======================================================================
        // IF EXPRESSION AS ASSIGNMENT: `x = if cond {`
        // Must handle BEFORE normal assignment to avoid adding semicolon
        // CRITICAL BUG B FIX: Must wrap in parentheses for valid Rust
        // RustS+: `x = if cond { a } else { b }`
        // Rust:   `let x = (if cond { a } else { b });`
        //=======================================================================
        if is_if_assignment(trimmed) {
            if let Some((var_name, if_expr)) = parse_control_flow_assignment(trimmed) {
                // CRITICAL FIX: Check if variable is a function parameter
                let is_param = current_fn_ctx.params.contains_key(&var_name);
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                // Use same logic: add let unless it's mutating a parameter
                let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
                
                // CRITICAL: Wrap if expression in parentheses for valid Rust expression context
                if needs_let {
                    let keyword = if needs_mut { "let mut" } else { "let" };
                    output_lines.push(format!("{}{} {} = ({}", leading_ws, keyword, var_name, if_expr));
                } else {
                    output_lines.push(format!("{}{} = ({}", leading_ws, var_name, if_expr));
                }
                // Track that we're in an if expression assignment
                if_expr_assignment_depth = Some(prev_depth);
                continue;
            }
        }
        
        // Check for if expression assignment end (closing `}` at right depth)
        if if_expr_assignment_depth.is_some() && trimmed == "}" {
            let start_depth = if_expr_assignment_depth.unwrap();
            // Check if this } is at the same level as where if started
            // and not an else follows
            let next_is_else = {
                let mut found_else = false;
                for future_line in lines.iter().skip(line_num + 1) {
                    let ft = strip_inline_comment(future_line).trim().to_string();
                    if ft.is_empty() { continue; }
                    found_else = ft.starts_with("else") || ft.starts_with("} else");
                    break;
                }
                found_else
            };
            
            if brace_depth <= start_depth && !next_is_else {
                if_expr_assignment_depth = None;
                // CRITICAL: Close with }); to complete parenthesized expression
                output_lines.push(format!("{}}}); ", leading_ws));
                continue;
            }
        }
        
        // Inside match arm body - process lines including assignments
        if match_mode.in_arm_body() {
            // First check if it's a RustS+ assignment (including outer/mut keyword)
            if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
                let is_decl = scope_analyzer.is_decl(line_num);
                let is_mutation = scope_analyzer.is_mut(line_num);
                let borrowed_mut = tracker.is_mut_borrowed(&var_name);
                let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
                // CRITICAL: is_explicit_mut ALWAYS forces `let mut`
                let needs_mut = is_explicit_mut || borrowed_mut || scope_needs_mut;
                
                let mut expanded_value = expand_value(&value, var_type.as_deref());
                
                // L-04: Add .clone() for array access
                expanded_value = transform_array_access_clone(&expanded_value);
                
                if current_fn_ctx.is_inside() {
                    expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                }
                expanded_value = transform_call_args(&expanded_value, &fn_registry);
                
                // PRIORITY ORDER (FIXED):
                // 1. outer keyword → mutation to outer scope (no let)
                // 2. is_explicit_mut → new declaration with mut (let mut)
                // 3. is_decl from scope_analyzer → new declaration (let)
                // 4. is_mutation AND variable is a function parameter → mutation (no let)
                // 5. Default → new declaration (let)
                
                // CRITICAL FIX: Check if variable is a function parameter
                let is_param = current_fn_ctx.params.contains_key(&var_name);
                let is_shadowing = tracker.is_shadowing(&var_name, line_num);
                let should_have_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
                
                // CRITICAL FIX: Check if value ends with `{` (struct literal start)
                // If so, DON'T add semicolon and enter literal mode
                let is_struct_literal_start = expanded_value.trim().ends_with('{');
                let semi = if is_struct_literal_start { "" } else { ";" };
                
                // If this is a struct literal start inside match arm, enter literal mode
                if is_struct_literal_start {
                    // Enter literal mode for multi-line struct
                    literal_mode.enter(LiteralKind::Struct, prev_depth + opens, true);
                }
                
                if is_outer {
                    output_lines.push(format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi));
                } else if is_explicit_mut {
                    // CRITICAL: `mut x = 10` MUST become `let mut x = 10;`
                    let let_keyword = "let mut";
                    let type_annotation = if let Some(ref t) = var_type {
                        format!(": {}", t)
                    // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                    } else {
                        String::new()
                    };
                    output_lines.push(format!("{}{} {}{} = {}{}", 
                        leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi));
                } else if should_have_let {
                    // NEW DECLARATION: scope_analyzer says decl, or not mutation, or shadowing
                    let let_keyword = if needs_mut { "let mut" } else { "let" };
                    let type_annotation = if let Some(ref t) = var_type {
                        format!(": {}", t)
                    // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                    } else {
                        String::new()
                    };
                    output_lines.push(format!("{}{} {}{} = {}{}", 
                        leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi));
                } else if is_mutation && is_param {
                    // MUTATION: Only for actual mutations of function parameters
                    output_lines.push(format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi));
                } else {
                    // FALLBACK: Default to new declaration
                    let let_keyword = if needs_mut { "let mut" } else { "let" };
                    let type_annotation = if let Some(ref t) = var_type {
                        format!(": {}", t)
                    // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                    } else {
                        String::new()
                    };
                    output_lines.push(format!("{}{} {}{} = {}{}", 
                        leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi));
                }
                continue;
            }
            
            //=================================================================
            // BARE STRUCT LITERAL INSIDE MATCH ARM BODY
            // Handle `Wallet {` as return expression (no assignment prefix)
            // Must check BEFORE "Not an assignment" block!
            //=================================================================
            if let Some(struct_name) = detect_bare_struct_literal(trimmed, &struct_registry) {
                // Check if single-line: `Wallet { id: 1, balance: 100 }`
                if trimmed.ends_with('}') {
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                    continue;
                }
                
                // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
                literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
                output_lines.push(format!("{}{} {{", leading_ws, struct_name));
                continue;
            }
            
            //=================================================================
            // BARE ENUM STRUCT VARIANT LITERAL INSIDE MATCH ARM BODY
            // Handle `Event::Data {` as return expression (no assignment prefix)
            //=================================================================
            if let Some(enum_path) = detect_bare_enum_literal(trimmed) {
                // Check if single-line
                if trimmed.ends_with('}') {
                    let transformed = transform_bare_struct_literal(trimmed);
                    output_lines.push(format!("{}{}", leading_ws, transformed));
                    continue;
                }
                
                // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
                literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
                output_lines.push(format!("{}{} {{", leading_ws, enum_path));
                continue;
            }
            
            // Not an assignment - apply transformations
            let mut transformed = trimmed.to_string();
            
            //=================================================================
            // L-01 CRITICAL FIX: Handle bare `mut x = value` in match arm body
            // If parse_rusts_assignment_ext failed but line starts with `mut `,
            // this is a bug - we should force proper transformation
            //=================================================================
            if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
                // This is a bare `mut x = value` that should have been transformed!
                let rest = trimmed.strip_prefix("mut ").unwrap().trim();
                if let Some(eq_pos) = rest.find('=') {
                    let var_part = rest[..eq_pos].trim();
                    let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
                    
                    let (var_name, type_annotation) = if var_part.contains(':') {
                        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            (parts[0].trim(), format!(": {}", parts[1].trim()))
                        } else {
                            (var_part, String::new())
                        }
                    } else {
                        (var_part, String::new())
                    };
                    
                    let mut expanded_value = expand_value(val_part, None);
                    expanded_value = transform_array_access_clone(&expanded_value);
                    if current_fn_ctx.is_inside() {
                        expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                    }
                    expanded_value = transform_call_args(&expanded_value, &fn_registry);
                    
                    let output = format!("{}let mut {}{} = {};", 
                        leading_ws, var_name, type_annotation, expanded_value);
                    output_lines.push(output);
                    continue;
                }
            }
            
            // Apply function context transformations
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
            // Check if this is a tail expression (last line before `}`)
            let is_tail = is_before_closing_brace;
            
            // String literal as tail expression in String-returning function
            if is_tail {
                if let Some(ref ret_type) = current_fn_ctx.return_type {
                    if ret_type == "String" && control_flow::is_string_literal(&transformed) {
                        transformed = control_flow::transform_string_to_owned(&transformed);
                    }
                }
            }
            
            // Add semicolon if needed (but not for tail expressions)
            if needs_semicolon(&transformed) && !is_tail {
                output_lines.push(format!("{}{};", leading_ws, transformed));
            } else {
                output_lines.push(format!("{}{}", leading_ws, transformed));
            }
            continue;
        }
        
        //=======================================================================
        // STRUCT DEFINITION (type definition, not instantiation)
        // L-12: Always inject #[derive(Clone)] for RustS+ value semantics
        // RustS+ has value semantics - all types should be cloneable
        //=======================================================================
        if is_struct_definition(trimmed) && !in_struct_def {
            in_struct_def = true;
            struct_def_depth = brace_depth;
            
            // L-12: Always add Clone for RustS+ structs
            if let Some(_struct_name) = parse_struct_header(trimmed) {
                // Check if previous line already has #[derive(...)]
                let prev_line = output_lines.last().map(|s| s.trim().to_string());
                if let Some(ref prev) = prev_line {
                    if prev.starts_with("#[derive(") && prev.ends_with(")]") {
                        // Check if Clone is already present
                        if !prev.contains("Clone") {
                            // Merge Clone into existing derive
                            let last_idx = output_lines.len() - 1;
                            let existing = output_lines[last_idx].clone();
                            let merged = existing.replace(")]", ", Clone)]");
                            output_lines[last_idx] = merged;
                        }
                    } else {
                        // Add new derive line
                        output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                    }
                } else {
                    output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                }
            }
            
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        if in_struct_def {
            if trimmed == "}" && brace_depth <= struct_def_depth {
                in_struct_def = false;
                output_lines.push(format!("{}}}", leading_ws));
                continue;
            }
            let transformed = transform_struct_field(&clean_line);
            // CRITICAL FIX: Transform bare slice types [T] to Vec<T> for struct fields
            // Bare slices are unsized and can't be struct fields in Rust
            let transformed = transform_struct_field_slice_to_vec(&transformed);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // ENUM DEFINITION
        // L-12: Always inject #[derive(Clone)] for RustS+ value semantics
        //=======================================================================
        if is_enum_definition(trimmed) && !enum_ctx.in_enum_def {
            enum_ctx.enter_enum(brace_depth);
            
            // L-12: Always add Clone for RustS+ enums
            if let Some(_enum_name) = parse_enum_header(trimmed) {
                // Check if previous line already has #[derive(...)]
                let prev_line = output_lines.last().map(|s| s.trim().to_string());
                if let Some(ref prev) = prev_line {
                    if prev.starts_with("#[derive(") && prev.ends_with(")]") {
                        // Check if Clone is already present
                        if !prev.contains("Clone") {
                            // Merge Clone into existing derive
                            let last_idx = output_lines.len() - 1;
                            let existing = output_lines[last_idx].clone();
                            let merged = existing.replace(")]", ", Clone)]");
                            output_lines[last_idx] = merged;
                        }
                    } else {
                        // Add new derive line
                        output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                    }
                } else {
                    output_lines.push(format!("{}#[derive(Clone)]", leading_ws));
                }
            }
            
            output_lines.push(format!("{}{}", leading_ws, trimmed));
            continue;
        }
        
        if enum_ctx.in_enum_def {
            // Check struct variant exit FIRST (before enum exit check)
            if trimmed == "}" && enum_ctx.in_struct_variant {
                enum_ctx.exit_struct_variant();
                output_lines.push(format!("{}}},", leading_ws));
                continue;
            }
            
            // Then check enum exit
            if trimmed == "}" && brace_depth <= enum_ctx.start_depth {
                enum_ctx.exit_enum();
                output_lines.push(format!("{}}}", leading_ws));
                continue;
            }
            
            // Struct variant start tracking
            // CRITICAL FIX: Use opens > closes to detect multi-line struct variants
            // A line like `Variant { x: i32 },` has opens=closes=1, so it's complete
            if trimmed.contains('{') && opens > closes {
                enum_ctx.enter_struct_variant();
            }
            
            let transformed = transform_enum_variant(&clean_line, enum_ctx.in_struct_variant);
            output_lines.push(transformed);
            continue;
        }
        
        //=======================================================================
        // STRUCT LITERAL START: `u = User {`
        // ENTER LITERAL MODE - NO let inside!
        // L-07: Check if variable needs mut (reassigned in child scope)
        //=======================================================================
        if let Some((var_name, struct_name)) = detect_struct_literal_start(trimmed, &struct_registry) {
            // L-07: Determine if this binding needs mut
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut;
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            
            // Check if single-line: `u = User { id = 1 }`
            if trimmed.ends_with('}') {
                let transformed = transform_single_line_struct_literal(trimmed, &var_name);
                // L-07: Replace "let " with appropriate keyword
                let transformed = if needs_mut {
                    transformed.replacen("let ", "let mut ", 1)
                } else {
                    transformed
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line struct literal - ENTER LITERAL MODE (is_assignment = true)
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, true);
            output_lines.push(format!("{}{} {} = {} {{", leading_ws, let_keyword, var_name, struct_name));
            continue;
        }
        
        //=======================================================================
        // ENUM STRUCT VARIANT LITERAL: `e = Event::Data {`
        // ENTER LITERAL MODE - NO let inside!
        // L-07: Check if variable needs mut (reassigned in child scope)
        //=======================================================================
        if let Some((var_name, enum_path)) = detect_enum_literal_start(trimmed) {
            // L-07: Determine if this binding needs mut
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let needs_mut = scope_needs_mut || borrowed_mut;
            let let_keyword = if needs_mut { "let mut" } else { "let" };
            
            // Check if single-line
            if trimmed.ends_with('}') {
                let transformed = transform_single_line_enum_literal(trimmed, &var_name, &enum_path);
                // L-07: Replace "let " with appropriate keyword
                let transformed = if needs_mut {
                    transformed.replacen("let ", "let mut ", 1)
                } else {
                    transformed
                };
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = true)
            literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, true);
            output_lines.push(format!("{}{} {} = {} {{", leading_ws, let_keyword, var_name, enum_path));
            continue;
        }
        
        //=======================================================================
        // BARE STRUCT LITERAL (return expression): `Packet {`
        // ENTER LITERAL MODE - NO let, this is a return expression!
        //=======================================================================
        if let Some(struct_name) = detect_bare_struct_literal(trimmed, &struct_registry) {
            // Check if single-line: `Packet { header: h, payload: p }`
            if trimmed.ends_with('}') {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
            literal_mode.enter(LiteralKind::Struct, prev_depth + opens, false);
            output_lines.push(format!("{}{} {{", leading_ws, struct_name));
            continue;
        }
        
        //=======================================================================
        // BARE ENUM STRUCT VARIANT LITERAL: `Event::Data {`
        // ENTER LITERAL MODE - NO let, this is a return expression!
        //=======================================================================
        if let Some(enum_path) = detect_bare_enum_literal(trimmed) {
            // Check if single-line
            if trimmed.ends_with('}') {
                let transformed = transform_bare_struct_literal(trimmed);
                output_lines.push(format!("{}{}", leading_ws, transformed));
                continue;
            }
            
            // Multi-line - ENTER LITERAL MODE (is_assignment = false, return expression)
            literal_mode.enter(LiteralKind::EnumVariant, prev_depth + opens, false);
            output_lines.push(format!("{}{} {{", leading_ws, enum_path));
            continue;
        }
        
        //=======================================================================
        // FUNCTION DEFINITION
        //=======================================================================
        if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") {
            match parse_function_line(trimmed) {
                FunctionParseResult::RustSPlusSignature(sig) => {
                    let rust_sig = signature_to_rust(&sig);
                    output_lines.push(format!("{}{}", leading_ws, rust_sig));
                    continue;
                }
                FunctionParseResult::RustPassthrough => {
                    // L-05 CRITICAL FIX: Even for Rust passthrough, strip effects clause
                    // User may write: fn foo(a: i32) effects(io) { }
                    // which has Rust-style params but RustS+ effects
                    let mut output = clean_line.clone();
                    
                    // Strip effects annotation if present
                    if output.contains("effects(") {
                        // Find and remove effects(...) clause
                        // Pattern: "effects(" ... ")" followed by optional whitespace and then rest
                        if let Some(effects_start) = output.find("effects(") {
                            // Find the matching closing paren
                            let mut paren_depth = 0;
                            let mut effects_end = effects_start;
                            for (i, c) in output[effects_start..].char_indices() {
                                match c {
                                    '(' => paren_depth += 1,
                                    ')' => {
                                        paren_depth -= 1;
                                        if paren_depth == 0 {
                                            effects_end = effects_start + i + 1;
                                            break;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            // Remove the effects clause
                            let before = &output[..effects_start];
                            let after = output[effects_end..].trim_start();
                            output = format!("{}{}", before.trim_end(), 
                                if after.is_empty() || after.starts_with('{') { 
                                    format!(" {}", after) 
                                } else { 
                                    format!(" {}", after) 
                                });
                        }
                    }
                    
                    // Extract return type for tail return handling
                    if output.contains(" -> ") {
                        if let Some(arrow_pos) = output.find(" -> ") {
                            let after_arrow = &output[arrow_pos + 4..];
                            let ret_end = after_arrow.find(|c: char| c == '{' || c.is_whitespace())
                                .unwrap_or(after_arrow.len());
                            let ret_type = after_arrow[..ret_end].trim();
                            if !ret_type.is_empty() {
                                current_fn_ctx.return_type = Some(ret_type.to_string());
                                current_fn_ctx.start_depth = function_start_brace;
                            }
                        }
                    }
                    if needs_semicolon(trimmed) {
                        output = format!("{};", output);
                    }
                    output_lines.push(output);
                    continue;
                }
                FunctionParseResult::Error(e) => {
                    output_lines.push(format!("{}// COMPILE ERROR: {}", leading_ws, e));
                    output_lines.push(clean_line.clone());
                    continue;
                }
                FunctionParseResult::NotAFunction => {
                    output_lines.push(clean_line.clone());
                    continue;
                }
            }
        }
        
        //=======================================================================
        // L-07: EFFECT STATEMENT SKIP
        // `effect write(account)` is a RustS+-only declaration
        // It must NOT appear in Rust output - skip entirely
        //=======================================================================
        if trimmed.starts_with("effect ") {
            // Effect statements are purely for the effect ownership system
            // They do not produce any Rust output
            continue;
        }
        
        //=======================================================================
        // RUST NATIVE PASSTHROUGH
        //=======================================================================
        let is_rust_native = trimmed.starts_with("let ") 
            || trimmed.starts_with("const ") 
            || trimmed.starts_with("static ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("mod ") 
            || trimmed.starts_with("impl ") 
            || trimmed.starts_with("trait ") 
            || trimmed.starts_with("type ")
            || trimmed.starts_with("//") 
            || trimmed.starts_with("/*") 
            || trimmed.starts_with("*")
            || trimmed.starts_with('#') 
            || trimmed == "{" 
            || trimmed == "}"
            || trimmed.starts_with("if ")
            || trimmed.starts_with("else")
            || trimmed.starts_with("for ")
            || trimmed.starts_with("while ")
            || trimmed.starts_with("loop")
            || trimmed.starts_with("match ")
            || trimmed.starts_with("return ")
            || trimmed.starts_with("break")
            || trimmed.starts_with("continue")
            || trimmed.starts_with("pub ");
        
        if is_rust_native {
            let mut transformed = trimmed.to_string();
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
                transformed = transform_call_args(&transformed, &fn_registry);
            }
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            transformed = transform_enum_struct_init(&transformed);
            
            let is_return_expr = should_be_tail_return(&transformed, &current_fn_ctx, is_before_closing_brace);
            
            let output = if needs_semicolon(&transformed) && !is_return_expr {
                format!("{}{};", leading_ws, transformed)
            } else {
                format!("{}{}", leading_ws, transformed)
            };
            
            output_lines.push(output);
            continue;
        }
        
        //=======================================================================
        // ARRAY LITERAL START: `events = [` (multiline)
        // ENTER ARRAY MODE - NO semicolons inside!
        //=======================================================================
        if let Some((var_name, var_type, after_bracket)) = detect_array_literal_start(trimmed) {
            // CRITICAL FIX: Check if variable is a function parameter
            let is_param = current_fn_ctx.params.contains_key(&var_name);
            let is_decl = scope_analyzer.is_decl(line_num);
            let is_mutation = scope_analyzer.is_mut(line_num);
            let is_shadowing = tracker.is_shadowing(&var_name, line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            let needs_mut = borrowed_mut || scope_needs_mut;
            // Use same logic: add let unless it's mutating a parameter
            let needs_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
            
            // Enter array mode
            array_mode.enter(
                prev_bracket_depth + bracket_opens, 
                true, // is_assignment
                var_name.clone(),
                var_type.clone(),
                needs_let,
                needs_mut
            );
            
            // Emit the opening line
            let let_keyword = if needs_let {
                if needs_mut { "let mut " } else { "let " }
            } else {
                ""
            };
            
            let type_annotation = if let Some(ref t) = var_type {
                format!(": {}", t)
            } else {
                String::new()
            };
            
            // Check if there's content after the [
            let after = after_bracket.trim();
            if after.is_empty() {
                output_lines.push(format!("{}{}{}{} = [", leading_ws, let_keyword, var_name, type_annotation));
            } else {
                // Has first element on same line
                let transformed_first = transform_array_element(&format!("    {}", after));
                output_lines.push(format!("{}{}{}{} = [", leading_ws, let_keyword, var_name, type_annotation));
                if !transformed_first.trim().is_empty() {
                    output_lines.push(transformed_first);
                }
            }
            continue;
        }
        
        //=======================================================================
        // RUSTS+ ASSIGNMENT (normal variable assignment, NOT in literal mode)
        //=======================================================================
        if let Some((var_name, var_type, value, is_outer, is_explicit_mut)) = parse_rusts_assignment_ext(&clean_line) {
            let is_decl = scope_analyzer.is_decl(line_num);
            let is_mutation = scope_analyzer.is_mut(line_num);
            let borrowed_mut = tracker.is_mut_borrowed(&var_name);
            let scope_needs_mut = scope_analyzer.needs_mut(&var_name, line_num);
            // CRITICAL: is_explicit_mut ALWAYS forces `let mut`
            let needs_mut = is_explicit_mut || borrowed_mut || scope_needs_mut;
            
            let mut expanded_value = expand_value(&value, var_type.as_deref());
            
            //===================================================================
            // L-04: Non-Copy Array/Collection Access
            // If RHS is array indexing like `arr[i]`, add .clone() for non-Copy
            // Heuristic: any array index access gets .clone() unless it's a
            // primitive numeric/bool literal context
            //===================================================================
            expanded_value = transform_array_access_clone(&expanded_value);
            
            if current_fn_ctx.is_inside() {
                expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
            }
            expanded_value = transform_call_args(&expanded_value, &fn_registry);
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            expanded_value = transform_enum_struct_init(&expanded_value);
            
            // PRIORITY ORDER (FIXED):
            // 1. outer keyword → mutation to outer scope (no let)
            // 2. is_explicit_mut → new declaration with mut (let mut)
            // 3. is_decl from scope_analyzer → new declaration (let)
            // 4. is_mutation AND variable is a function parameter → mutation (no let)
            // 5. Default → new declaration (let) - safer than causing "cannot find value" errors
            
            // CRITICAL FIX: Check if variable is a function parameter
            // Only treat as mutation if it's mutating a parameter, not a local variable
            let is_param = current_fn_ctx.params.contains_key(&var_name);
            let is_shadowing = tracker.is_shadowing(&var_name, line_num);
            
            // CRITICAL: is_mutation should only skip `let` if it's actually mutating
            // a previously declared variable in the SAME scope
            // If scope_analyzer marked it as decl, trust that
            // If neither decl nor mutation, default to adding `let`
            let should_have_let = is_decl || (!is_mutation && !is_param) || is_shadowing;
            
            if is_outer {
                // Check if value ends with continuation operator OR method chain OR inside multiline expr
                let semi = if ends_with_continuation_operator(&expanded_value) || next_line_is_method_chain || inside_multiline_expr { "" } else { ";" };
                let output_line = format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi);
                output_lines.push(output_line);
                prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
            } else if is_explicit_mut {
                // CRITICAL: `mut x = 10` MUST become `let mut x = 10;`
                let let_keyword = "let mut";
                let type_annotation = if let Some(ref t) = var_type {
                    format!(": {}", t)
                // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                } else {
                    String::new()
                };
                // Check if value ends with continuation operator OR method chain OR inside multiline expr
                let semi = if ends_with_continuation_operator(&expanded_value) || next_line_is_method_chain || inside_multiline_expr { "" } else { ";" };
                let output_line = format!("{}{} {}{} = {}{}", 
                    leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi);
                output_lines.push(output_line);
                prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
            } else if should_have_let {
                // NEW DECLARATION: Either scope_analyzer says decl, or it's not a mutation, or shadowing
                let let_keyword = if needs_mut { "let mut" } else { "let" };
                let type_annotation = if let Some(ref t) = var_type {
                    format!(": {}", t)
                // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                } else {
                    String::new()
                };
                // Check if value ends with continuation operator OR method chain OR inside multiline expr
                let semi = if ends_with_continuation_operator(&expanded_value) || next_line_is_method_chain || inside_multiline_expr { "" } else { ";" };
                let output_line = format!("{}{} {}{} = {}{}", 
                    leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi);
                output_lines.push(output_line);
                prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
            } else if is_mutation && is_param {
                // MUTATION: Only for actual mutations of function parameters
                let semi = if ends_with_continuation_operator(&expanded_value) || next_line_is_method_chain || inside_multiline_expr { "" } else { ";" };
                let output_line = format!("{}{} = {}{}", leading_ws, var_name, expanded_value, semi);
                output_lines.push(output_line);
                prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
            } else {
                // FALLBACK: Default to new declaration (safer)
                let let_keyword = if needs_mut { "let mut" } else { "let" };
                let type_annotation = if let Some(ref t) = var_type {
                    format!(": {}", t)
                // FIXED: Don't auto-add `: String` - let Rust infer &str for bare literals
                } else {
                    String::new()
                };
                let semi = if ends_with_continuation_operator(&expanded_value) || next_line_is_method_chain || inside_multiline_expr { "" } else { ";" };
                let output_line = format!("{}{} {}{} = {}{}", 
                    leading_ws, let_keyword, var_name, type_annotation, expanded_value, semi);
                output_lines.push(output_line);
                prev_line_was_continuation = ends_with_continuation_operator(&expanded_value);
            }
        } else {
            // Not an assignment - transform and pass through
            let mut transformed = trimmed.to_string();
            
            //=================================================================
            // L-01 CRITICAL FIX: Handle bare `mut x = value` that wasn't parsed
            // If parse_rusts_assignment_ext failed but line starts with `mut `,
            // this is a bug - we should force proper transformation
            //=================================================================
            if trimmed.starts_with("mut ") && trimmed.contains('=') && !trimmed.contains("==") {
                // This is a bare `mut x = value` that should have been transformed!
                // Try to manually parse and transform it
                let rest = trimmed.strip_prefix("mut ").unwrap().trim();
                if let Some(eq_pos) = rest.find('=') {
                    let var_part = rest[..eq_pos].trim();
                    let val_part = rest[eq_pos + 1..].trim().trim_end_matches(';');
                    
                    // Check for type annotation
                    // CRITICAL: Check RustS+ style (space) FIRST before Rust style (colon)
                    // because colon might be part of :: in path like crate::types::Address
                    let (var_name, type_annotation) = if var_part.contains(' ') {
                        // RustS+ style: var Type (no colon separator)
                        // Split by first space to get var_name and type
                        let space_pos = var_part.find(' ').unwrap();
                        let vname = var_part[..space_pos].trim();
                        let vtype = var_part[space_pos + 1..].trim();
                        
                        // Validate: vname must be valid identifier, vtype must look like a type
                        let vname_valid = !vname.is_empty() 
                            && vname.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
                            && vname.chars().all(|c| c.is_alphanumeric() || c == '_');
                        
                        // Type typically starts with uppercase, or is a known generic like Vec[, Option[, etc.
                        // Also handle reference types like &Type, &mut Type
                        let vtype_valid = !vtype.is_empty() && (
                            vtype.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)
                            || vtype.starts_with("Vec[") || vtype.starts_with("Vec<")
                            || vtype.starts_with("Option[") || vtype.starts_with("Option<")
                            || vtype.starts_with("Result[") || vtype.starts_with("Result<")
                            || vtype.starts_with("HashMap[") || vtype.starts_with("HashMap<")
                            || vtype.starts_with("HashSet[") || vtype.starts_with("HashSet<")
                            || vtype.starts_with("BTreeMap[") || vtype.starts_with("BTreeMap<")
                            || vtype.starts_with("BTreeSet[") || vtype.starts_with("BTreeSet<")
                            || vtype.starts_with("Box[") || vtype.starts_with("Box<")
                            || vtype.starts_with("Arc[") || vtype.starts_with("Arc<")
                            || vtype.starts_with("Rc[") || vtype.starts_with("Rc<")
                            || vtype.starts_with('&')  // Reference types
                            || vtype.starts_with('(')  // Tuple types
                            || vtype.starts_with('[')  // Slice/array types
                            || vtype == "i8" || vtype == "i16" || vtype == "i32" || vtype == "i64" || vtype == "i128"
                            || vtype == "u8" || vtype == "u16" || vtype == "u32" || vtype == "u64" || vtype == "u128"
                            || vtype == "f32" || vtype == "f64"
                            || vtype == "bool" || vtype == "char" || vtype == "usize" || vtype == "isize"
                        );
                        
                        if vname_valid && vtype_valid {
                            (vname, format!(": {}", vtype))
                        } else {
                            (var_part, String::new())
                        }
                    } else if var_part.contains(':') && !var_part.contains("::") {
                        // Rust style: var: Type (only if colon is NOT part of ::)
                        let parts: Vec<&str> = var_part.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            (parts[0].trim(), format!(": {}", parts[1].trim()))
                        } else {
                            (var_part, String::new())
                        }
                    } else {
                        // No type annotation
                        (var_part, String::new())
                    };
                    
                    // Transform the value
                    let mut expanded_value = expand_value(val_part, None);
                    expanded_value = transform_array_access_clone(&expanded_value);
                    if current_fn_ctx.is_inside() {
                        expanded_value = transform_string_concat(&expanded_value, &current_fn_ctx);
                    }
                    expanded_value = transform_call_args(&expanded_value, &fn_registry);
                    
                    // Output as `let mut var = value;`
                    let output = format!("{}let mut {}{} = {};", 
                        leading_ws, var_name, type_annotation, expanded_value);
                    output_lines.push(output);
                    continue;
                }
            }
            
            if current_fn_ctx.is_inside() {
                transformed = transform_string_concat(&transformed, &current_fn_ctx);
            }
            transformed = transform_call_args(&transformed, &fn_registry);
            
            // Transform enum struct init: Event::C { x = 4 } -> Event::C { x: 4 }
            transformed = transform_enum_struct_init(&transformed);
            
            let is_return_expr = should_be_tail_return(&transformed, &current_fn_ctx, is_before_closing_brace);
            
            // CRITICAL: Convert string literal tail returns to String::from when return type is String
            if is_return_expr {
                if let Some(ref ret_type) = current_fn_ctx.return_type {
                    if ret_type == "String" && is_string_literal(&transformed) {
                        let inner = &transformed[1..transformed.len()-1];
                        transformed = format!("String::from(\"{}\")", inner);
                    }
                }
            }
            
            // Check if this is a continuation line or ends with continuation operator
            let this_line_is_continuation = prev_line_was_continuation;
            let this_line_ends_with_continuation = ends_with_continuation_operator(&transformed);
            
            // Update continuation state for next iteration
            prev_line_was_continuation = this_line_ends_with_continuation;
            
            // Determine semicolon: 
            // - No semicolon if line ends with continuation operator (expression continues on next line)
            // - No semicolon if it's a return expression (no ; needed before })
            // - No semicolon if inside a multiline expression (format!(...), vec![...], etc.)
            // - No semicolon if next line starts with `.` (method chain continuation)
            // - Otherwise, use needs_semicolon check
            // CRITICAL FIX: We no longer skip semicolon just because PREV line was continuation
            // Only skip if CURRENT line ends with continuation operator
            let should_add_semi = !this_line_ends_with_continuation
                && !is_return_expr 
                && !inside_multiline_expr  // CRITICAL: Skip semicolon inside multiline expressions
                && !next_line_is_method_chain  // CRITICAL: Skip semicolon for method chain continuation
                && needs_semicolon(&transformed);
            
            if should_add_semi {
                let output = format!("{}{};", leading_ws, transformed);
                output_lines.push(output);
            } else {
                let output = format!("{}{}", leading_ws, transformed);
                output_lines.push(output);
            }
        }
    }
    
    // L-08: Transform macro calls (println -> println!, etc.)
    let transformed_lines: Vec<String> = output_lines
        .into_iter()
        .map(|line| transform_macro_calls(&line))
        .collect();
    
    //==========================================================================
    // L-01 POST-PROCESSING FIX: Catch any remaining bare `mut x = value`
    // This is a safety net for cases that slipped through the main processing.
    // Convert `mut x = value` to `let mut x = value;`
    //==========================================================================
    let fixed_lines: Vec<String> = transformed_lines
        .into_iter()
        .map(|line| fix_bare_mut_declaration(&line))
        .collect();
    
    //==========================================================================
    // L-05 POST-PROCESSING FIX: Strip any remaining effect annotations
    // This catches effect annotations that may have leaked through other paths.
    //==========================================================================
    let final_lines: Vec<String> = fixed_lines
        .into_iter()
        .map(|line| strip_effects_from_line(&line))
        .collect();
    
    //==========================================================================
    // CRITICAL POST-PROCESSING: Strip `outer` keyword from field assignments
    // `outer self.field = value` → `self.field = value`
    // This handles cases where the assignment parser didn't match because
    // `self.field` isn't a valid simple identifier
    //==========================================================================
    let outer_stripped: Vec<String> = final_lines
        .into_iter()
        .map(|line| strip_outer_keyword(&line))
        .collect();
    
    //==========================================================================
    // CRITICAL POST-PROCESSING: Transform RustS+ generic syntax to Rust
    // `Vec[String]` → `Vec<String>`, `HashMap[K, V]` → `HashMap<K, V>`
    // This handles generic type annotations throughout the code
    //==========================================================================
    let generic_transformed: Vec<String> = outer_stripped
        .into_iter()
        .map(|line| transform_generic_brackets(&line))
        .collect();
    
    let result = generic_transformed.join("\n");
    
    //==========================================================================
    // L-05: RUST SANITY CHECK
    // Validate that generated Rust code is syntactically valid BEFORE output.
    // If invalid → panic with INTERNAL COMPILER ERROR (not rustc error)
    //==========================================================================
    #[cfg(not(test))]
    {
        let sanity = rust_sanity::check_rust_output(&result);
        if !sanity.is_valid {
            // In non-test mode, print error but still return the code
            // (actual error handling is in main.rs)
            eprintln!("{}", rust_sanity::format_internal_error(&sanity));
        }
    }
    
    result
}

/// Transform single-line struct literal: `u = User { id = 1, name = "x" }`
fn transform_single_line_struct_literal(line: &str, var_name: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(eq_pos) = trimmed.find('=') {
        let rhs = trimmed[eq_pos + 1..].trim();
        
        if let Some(brace_start) = rhs.find('{') {
            let struct_name = rhs[..brace_start].trim();
            let brace_end = rhs.rfind('}').unwrap_or(rhs.len());
            let fields_part = &rhs[brace_start + 1..brace_end];
            
            let transformed_fields = transform_literal_fields_inline(fields_part);
            
            return format!("let {} = {} {{ {} }};", var_name, struct_name, transformed_fields);
        }
    }
    
    format!("let {};", line)
}

/// Transform single-line enum literal: `e = Event::Data { id = 1 }`
fn transform_single_line_enum_literal(line: &str, var_name: &str, enum_path: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("let {} = {} {{ {} }};", var_name, enum_path, transformed_fields);
    }
    
    format!("let {};", line)
}

/// Transform BARE struct/enum literal (return expression): `Packet { header = h }`
/// NO let - this is a return expression!
fn transform_bare_struct_literal(line: &str) -> String {
    let trimmed = line.trim();
    
    if let Some(brace_start) = trimmed.find('{') {
        let name_part = trimmed[..brace_start].trim();
        let brace_end = trimmed.rfind('}').unwrap_or(trimmed.len());
        let fields_part = &trimmed[brace_start + 1..brace_end];
        
        let transformed_fields = transform_literal_fields_inline(fields_part);
        
        return format!("{} {{ {} }}", name_part, transformed_fields);
    }
    
    trimmed.to_string()
}

/// Transform inline literal fields: `id = 1, name = "x"` → `id: 1, name: String::from("x"),`
fn transform_literal_fields_inline(fields: &str) -> String {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_string = false;
    let mut brace_depth: usize = 0;
    
    // First pass: collect all fields
    let mut raw_fields = Vec::new();
    for c in fields.chars() {
        if c == '"' && !current.ends_with('\\') {
            in_string = !in_string;
        }
        if !in_string {
            if c == '{' { brace_depth += 1; }
            if c == '}' { brace_depth = brace_depth.saturating_sub(1); }
        }
        
        if c == ',' && !in_string && brace_depth == 0 {
            raw_fields.push(current.clone());
            current.clear();
        } else {
            current.push(c);
        }
    }
    if !current.trim().is_empty() {
        raw_fields.push(current);
    }
    
    // CRITICAL FIX: Track field values to detect duplicates
    // Duplicate values (like from.address used twice) need .clone() on earlier uses
    let mut value_last_index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    
    // First pass: find last occurrence of each value expression
    for (i, field) in raw_fields.iter().enumerate() {
        if let Some(val) = extract_field_value(field) {
            // Normalize the value for comparison
            let normalized = val.trim().to_string();
            if !normalized.is_empty() && is_moveable_expression(&normalized) {
                value_last_index.insert(normalized, i);
            }
        }
    }
    
    // Second pass: transform fields, adding .clone() for duplicate values
    for (i, field) in raw_fields.iter().enumerate() {
        let field_val = extract_field_value(field);
        let needs_clone = if let Some(ref val) = field_val {
            let normalized = val.trim().to_string();
            if let Some(&last_idx) = value_last_index.get(&normalized) {
                i < last_idx && is_moveable_expression(&normalized)
            } else {
                false
            }
        } else {
            false
        };
        
        let transformed = transform_single_literal_field_with_clone(field, needs_clone);
        if !transformed.is_empty() {
            result.push(transformed);
        }
    }
    
    result.join(", ")
}

/// Extract field value from a field assignment
fn extract_field_value(field: &str) -> Option<String> {
    let trimmed = field.trim();
    if trimmed.is_empty() || trimmed.starts_with("..") {
        return None;
    }
    
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let value = trimmed[eq_pos + 1..].trim();
        return Some(value.to_string());
    } else if trimmed.contains(':') && !trimmed.contains("::") {
        if let Some(colon_pos) = trimmed.find(':') {
            let value = trimmed[colon_pos + 1..].trim();
            return Some(value.to_string());
        }
    }
    None
}

/// Check if an expression is "moveable" (not Copy, could cause use-after-move)
fn is_moveable_expression(expr: &str) -> bool {
    let expr = expr.trim();
    // Skip literals and simple values that are likely Copy
    if expr.parse::<i64>().is_ok() || expr.parse::<f64>().is_ok() {
        return false;
    }
    if expr == "true" || expr == "false" {
        return false;
    }
    
    // Skip method calls (contains `()`)
    if expr.contains("()") {
        return false;
    }
    
    // Skip cast expressions (contains ` as `)
    if expr.contains(" as ") {
        return false;
    }
    
    // Skip path expressions like TxType::Stake
    if expr.contains("::") {
        return false;
    }
    
    // Field access patterns like `from.address` are likely to be moveable
    if expr.contains('.') {
        return true;
    }
    
    // Simple identifiers that are struct/enum types might be moveable
    // But we can't know for sure without type info, so be conservative
    false
}

/// Transform a single field with optional .clone()
fn transform_single_literal_field_with_clone(field: &str, add_clone: bool) -> String {
    let trimmed = field.trim();
    if trimmed.is_empty() { return String::new(); }
    
    // Spread syntax
    if trimmed.starts_with("..") { return trimmed.to_string(); }
    
    // Already transformed (has colon)
    if trimmed.contains(':') && !trimmed.contains("::") {
        if add_clone {
            // Find the value part and add .clone()
            if let Some(colon_pos) = trimmed.find(':') {
                let name = trimmed[..colon_pos].trim();
                let value = trimmed[colon_pos + 1..].trim();
                // Only add .clone() if value is a clonable expression
                if should_clone_field_value(value) && !value.ends_with(".clone()") {
                    return format!("{}: {}.clone()", name, value);
                }
            }
        }
        return trimmed.to_string();
    }
    
    if let Some(eq_pos) = find_field_eq(trimmed) {
        let name = trimmed[..eq_pos].trim();
        let value = trimmed[eq_pos + 1..].trim();
        
        if is_valid_field_name(name) {
            let mut transformed_value = if is_string_literal(value) {
                let inner = &value[1..value.len()-1];
                format!("String::from(\"{}\")", inner)
            } else {
                value.to_string()
            };
            
            // Add .clone() if needed for duplicate values OR for field access expressions
            // CRITICAL FIX: Consistent with transform_literal_field_with_ctx
            let needs_clone = add_clone || should_clone_field_value(&transformed_value);
            if needs_clone && !transformed_value.ends_with(".clone()") {
                transformed_value = format!("{}.clone()", transformed_value);
            }
            
            return format!("{}: {}", name, transformed_value);
        }
    }
    
    trimmed.to_string()
}

/// Transform a single field: `id = 1` → `id: 1`
fn transform_single_literal_field(field: &str) -> String {
    transform_single_literal_field_with_clone(field, false)
}

//=============================================================================
// L-01 POST-PROCESSING: Fix bare `mut x = value` declarations
// This is a safety net that catches any bare mut that slipped through.
//=============================================================================


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_let() {
        let input = "a = 10";
        let output = parse_rusts(input);
        assert!(output.contains("let a = 10;"));
    }

    #[test]
    fn test_auto_mut() {
        let input = "a = 10\na = 20";
        let output = parse_rusts(input);
        assert!(output.contains("let mut a = 10;"));
        assert!(output.contains("a = 20;"));
    }

    #[test]
    fn test_type_annotation() {
        let input = "a: i32 = 10";
        let output = parse_rusts(input);
        assert!(output.contains("let a: i32 = 10;"));
    }

    #[test]
    fn test_string_literal() {
        let input = r#"a = "hello""#;
        let output = parse_rusts(input);
        assert!(output.contains("String::from(\"hello\")"));
    }

    #[test]
    fn test_struct_literal_no_let_inside() {
        let input = r#"u = User {
    id = 1
    name = "kian"
}"#;
        let output = parse_rusts(input);
        // Should NOT have `let id` or `let name` inside
        assert!(!output.contains("let id"));
        assert!(!output.contains("let name"));
        // Should have proper field syntax
        assert!(output.contains("id: 1,"));
        assert!(output.contains("name: String::from(\"kian\"),"));
    }

    #[test]
    fn test_struct_literal_multiline() {
        let input = r#"u = User {
    id = 1
    name = "test"
}"#;
        let output = parse_rusts(input);
        assert!(output.contains("let u = User {"));
        assert!(output.contains("id: 1,"));
        assert!(output.contains("name: String::from(\"test\"),"));
        // Top-level struct literal closes with }; (statement)
        assert!(output.contains("};"));
    }
    
    #[test]
    fn test_enum_struct_variant_no_let() {
        let input = r#"m = Message::Move { x = 10, y = 20 }"#;
        let output = parse_rusts(input);
        assert!(!output.contains("let x"));
        assert!(!output.contains("let y"));
        assert!(output.contains("x: 10"));
        assert!(output.contains("y: 20"));
    }
    
    #[test]
    fn test_bare_struct_literal_in_match_arm() {
        // This is the bug case: bare struct literal as return expression in match arm
        // Should NOT have `let` inside the struct literal fields!
        let input = r#"struct Wallet {
    id u32
    balance i64
}

enum Transaction {
    Deposit { amount i64 }
}

fn apply_tx(w Wallet, tx Transaction) Wallet {
    match tx {
        Transaction::Deposit { amount } {
            Wallet {
                id = w.id
                balance = w.balance + amount
            }
        }
    }
}"#;
        let output = parse_rusts(input);
        // Should NOT have `let id` or `let balance` inside struct literal
        assert!(!output.contains("let id"), "Bug: 'let id' found in struct literal: {}", output);
        assert!(!output.contains("let balance"), "Bug: 'let balance' found in struct literal: {}", output);
        // Should have proper field syntax
        assert!(output.contains("id:"), "Missing 'id:' field syntax in output: {}", output);
        assert!(output.contains("balance:"), "Missing 'balance:' field syntax in output: {}", output);
    }
    
    //=========================================================================
    // ARRAY LITERAL TESTS
    //=========================================================================
    
    #[test]
    fn test_array_literal_multiline() {
        let input = r#"events = [
    1,
    2,
    3
]"#;
        let output = parse_rusts(input);
        // Should have proper array syntax
        assert!(output.contains("let events = ["));
        // Should NOT have semicolons inside array
        assert!(!output.contains("[;"));
        assert!(!output.contains("1;"));
        assert!(!output.contains("2;"));
        // Should close properly
        assert!(output.contains("];"));
    }
    
    #[test]
    fn test_array_literal_with_enum_variants() {
        let input = r#"events = [
    Event::Credit { id = 1, amount = 500 },
    Event::Debit { id = 2, amount = 200 },
    Event::Query(3)
]"#;
        let output = parse_rusts(input);
        // Should have proper array start
        assert!(output.contains("let events = ["));
        // Should transform enum struct fields
        assert!(output.contains("id: 1"));
        assert!(output.contains("amount: 500"));
        assert!(output.contains("id: 2"));
        assert!(output.contains("amount: 200"));
        // Should have tuple variant
        assert!(output.contains("Event::Query(3)"));
        // Should NOT have illegal patterns
        assert!(!output.contains("= [;"));
        assert!(!output.contains("[;"));
        // Should close properly
        assert!(output.contains("];"));
    }
    
    #[test]
    fn test_array_literal_single_line_unchanged() {
        let input = "arr = [1, 2, 3]";
        let output = parse_rusts(input);
        // Single-line arrays should be handled by normal assignment
        assert!(output.contains("let arr = [1, 2, 3];"));
    }
    
    //=========================================================================
    // BUG FIX TESTS - Critical regression tests for the three main bugs
    //=========================================================================
    
    /// BUG A: `mut x = expr` MUST become `let mut x = expr;`
    #[test]
    fn test_bug_a_mut_lowering() {
        let input = "mut i = 0\ni = i + 1";
        let output = parse_rusts(input);
        // MUST have `let mut`
        assert!(output.contains("let mut i = 0;"), 
            "BUG A: Expected 'let mut i = 0;', got: {}", output);
        // Reassignment should work
        assert!(output.contains("i = i + 1;"),
            "BUG A: Expected 'i = i + 1;', got: {}", output);
        // Should NOT have bare `mut i` without `let`
        assert!(!output.lines().any(|l| l.trim().starts_with("mut i")), 
            "BUG A: Found bare 'mut i' without 'let': {}", output);
    }
    
    /// BUG A: `mut x: Type = expr` MUST become `let mut x: Type = expr;`
    #[test]
    fn test_bug_a_mut_with_type() {
        let input = "mut counter: i32 = 0";
        let output = parse_rusts(input);
        assert!(output.contains("let mut counter: i32 = 0;"), 
            "BUG A: Expected 'let mut counter: i32 = 0;', got: {}", output);
    }
    
    /// BUG B: `x = if cond { a } else { b }` MUST be parenthesized
    #[test]
    fn test_bug_b_if_expr_parenthesization() {
        let input = r#"x = if true {
    1
} else {
    2
}"#;
        let output = parse_rusts(input);
        // MUST have opening paren after =
        assert!(output.contains("= (if"), 
            "BUG B: Expected '= (if', got: {}", output);
        // MUST have closing });
        assert!(output.contains("});"), 
            "BUG B: Expected '}});', got: {}", output);
    }
    
    /// BUG C: `match String { "str" { ... } }` MUST add `.as_str()`
    #[test]
    fn test_bug_c_match_string_as_str() {
        let input = r#"match status {
    "rich" {
        println!("ok")
    }
    _ {
        println!("not ok")
    }
}"#;
        let output = parse_rusts(input);
        // MUST have .as_str() added to match expression
        assert!(output.contains("status.as_str()"), 
            "BUG C: Expected 'status.as_str()', got: {}", output);
    }
    
    /// BUG C: Non-string patterns should NOT add .as_str()
    #[test]
    fn test_bug_c_match_without_string_patterns() {
        let input = r#"match x {
    0 {
        println!("zero")
    }
    _ {
        println!("other")
    }
}"#;
        let output = parse_rusts(input);
        // Should NOT have .as_str() for non-string patterns
        assert!(!output.contains(".as_str()"), 
            "BUG C: Should not add .as_str() for non-string patterns: {}", output);
    }
    
    //=========================================================================
    // 5 ATURAN LOWERING FINAL - COMPREHENSIVE REGRESSION TESTS
    //=========================================================================
    
    /// L-01: `mut x = expr` WAJIB menjadi `let mut x = expr;`
    #[test]
    fn test_l01_mut_lowering_basic() {
        let input = "mut x = 0";
        let output = parse_rusts(input);
        assert!(output.contains("let mut x = 0;"), 
            "L-01: 'mut x = 0' must become 'let mut x = 0;', got: {}", output);
        // MUST NOT have bare `mut` 
        assert!(!output.lines().any(|l| {
            let t = l.trim();
            t.starts_with("mut ") && !l.contains("let mut") && !l.contains("&mut")
        }), "L-01: Found bare 'mut' without 'let': {}", output);
    }
    
    /// L-01: `mut x = expr` followed by `x = x + 1` WAJIB menjadi proper mut binding
    #[test]
    fn test_l01_mut_with_reassignment() {
        let input = "mut x = 0\nx = x + 1";
        let output = parse_rusts(input);
        assert!(output.contains("let mut x = 0;"), 
            "L-01: First line must be 'let mut x = 0;', got: {}", output);
        assert!(output.contains("x = x + 1;"), 
            "L-01: Second line must be 'x = x + 1;', got: {}", output);
        // Second line must NOT have `let`
        let lines: Vec<&str> = output.lines().collect();
        let second_line = lines.iter().find(|l| l.contains("x + 1")).unwrap();
        assert!(!second_line.contains("let"), 
            "L-01: Reassignment must not have 'let': {}", second_line);
    }
    
    /// L-02: Expression in match arm MUST be parenthesized
    #[test]
    fn test_l02_match_arm_if_expression() {
        let input = r#"match ev {
    A {
        if cond {
            1
        } else {
            2
        }
    }
}"#;
        let output = parse_rusts(input);
        // L-09: Match arms with if expressions use plain `=>`
        // The if expression is a valid expression context, no parens needed
        assert!(output.contains("=>") && output.contains("if cond"),
            "L-02/L-09: Match arm with if expr must use => format: {}", output);
    }
    
    /// L-03: Reassignment to same variable MUST use mut binding, not shadow
    #[test]
    fn test_l03_reassignment_uses_mut() {
        let input = "x = 10\nx = x + 1";
        let output = parse_rusts(input);
        // First line MUST have `let mut`
        assert!(output.contains("let mut x = 10;"), 
            "L-03: 'x = 10' followed by reassignment must be 'let mut x = 10;', got: {}", output);
        // Second line MUST NOT have `let` (it's reassignment, not new declaration)
        let lines: Vec<&str> = output.lines().collect();
        let second_line = lines.iter().find(|l| l.contains("x + 1")).unwrap();
        assert!(!second_line.contains("let"), 
            "L-03: Reassignment must not create new binding: {}", second_line);
    }
    
    /// L-03: Multiple reassignments must all work correctly
    #[test]
    fn test_l03_multiple_reassignments() {
        let input = "count = 0\ncount = count + 1\ncount = count + 2";
        let output = parse_rusts(input);
        assert!(output.contains("let mut count = 0;"), 
            "L-03: Initial assignment must be 'let mut', got: {}", output);
        // Count occurrences of "let" - should be exactly 1
        let let_count = output.matches("let ").count();
        assert_eq!(let_count, 1, 
            "L-03: Should have exactly 1 'let', got {}: {}", let_count, output);
    }
    
    /// L-04: Array access on non-Copy type MUST use .clone()
    #[test]
    fn test_l04_array_access_clone() {
        let input = "ev = events[i]";
        let output = parse_rusts(input);
        assert!(output.contains("events[i].clone()"), 
            "L-04: Array access must add .clone(): {}", output);
    }
    
    /// L-04: Array access that already has method call should NOT add .clone()
    #[test]
    fn test_l04_array_access_with_method() {
        let input = "len = items[0].len()";
        let output = parse_rusts(input);
        // Should NOT double-add clone
        assert!(!output.contains(".clone().clone()"), 
            "L-04: Should not double-clone: {}", output);
    }
    
    /// L-04: Simple numeric literals should NOT get .clone()
    #[test]
    fn test_l04_numeric_no_clone() {
        let input = "x = 42";
        let output = parse_rusts(input);
        assert!(!output.contains(".clone()"), 
            "L-04: Numeric literal should not get .clone(): {}", output);
    }
    
    /// L-05: Generated Rust output must have balanced delimiters
    #[test]
    fn test_l05_balanced_delimiters() {
        let input = r#"fn test() {
    x = 10
    if true {
        y = 20
    }
}"#;
        let output = parse_rusts(input);
        // Count braces
        let open_braces = output.matches('{').count();
        let close_braces = output.matches('}').count();
        assert_eq!(open_braces, close_braces, 
            "L-05: Braces must be balanced ({} open, {} close): {}", 
            open_braces, close_braces, output);
    }
    
    /// L-05: No bare `mut` tokens in output
    #[test]
    fn test_l05_no_bare_mut() {
        let input = "mut x = 10\nmut y: i32 = 20";
        let output = parse_rusts(input);
        // Check each line for bare mut
        for line in output.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("mut ") && !line.contains("let mut") && !line.contains("&mut") {
                panic!("L-05: Found bare 'mut' token: {}", line);
            }
        }
    }
    
    /// L-05: Effect annotations must NOT appear in Rust output
    #[test]
    fn test_l05_no_effect_leakage_rustsplus_syntax() {
        // RustS+ syntax function with effects
        let input = r#"fn apply_tx(w Wallet, tx Tx) effects(write w) Wallet {
    w
}"#;
        let output = parse_rusts(input);
        // MUST NOT have effects clause in output
        assert!(!output.contains("effects("), 
            "L-05: effects() must NOT appear in Rust output: {}", output);
        // MUST have return type
        assert!(output.contains("-> Wallet"), 
            "L-05: Return type 'Wallet' must be present: {}", output);
    }
    
    /// L-05: Effect annotations must be stripped from Rust-style params too
    #[test]
    fn test_l05_no_effect_leakage_rust_syntax() {
        // Rust-style params BUT with RustS+ effects
        let input = r#"fn log(msg: String) effects(io) {
    println!("{}", msg)
}"#;
        let output = parse_rusts(input);
        // MUST NOT have effects clause in output
        assert!(!output.contains("effects("), 
            "L-05: effects() must NOT appear in Rust output even with Rust-style params: {}", output);
    }
    
    /// L-05: Effect annotations with multiple effects must be stripped
    #[test]
    fn test_l05_no_effect_leakage_multiple_effects() {
        let input = r#"fn transfer(from Account, to Account) effects(read from, write to) Account {
    to
}"#;
        let output = parse_rusts(input);
        assert!(!output.contains("effects("), 
            "L-05: Multiple effects must be stripped: {}", output);
        assert!(output.contains("-> Account"), 
            "L-05: Return type must be preserved: {}", output);
    }
    
    /// L-01: mut with function call value must work
    #[test]
    fn test_l01_mut_with_function_call() {
        let input = "mut wallet = create_wallet(seed)";
        let output = parse_rusts(input);
        assert!(output.contains("let mut wallet = create_wallet(seed)"), 
            "L-01: 'mut wallet = create_wallet(seed)' must have 'let mut': {}", output);
        // MUST NOT have bare `mut`
        assert!(!output.lines().any(|l| {
            let t = l.trim();
            t.starts_with("mut ") && !l.contains("let mut") && !l.contains("&mut")
        }), "L-01: Found bare 'mut' without 'let': {}", output);
    }
    
    /// L-01: mut with array access must work
    #[test]
    fn test_l01_mut_with_array_access() {
        let input = "mut root = tx_hashes[0]";
        let output = parse_rusts(input);
        assert!(output.contains("let mut root"), 
            "L-01: 'mut root = tx_hashes[0]' must have 'let mut': {}", output);
        // Should also have .clone() due to L-04
        assert!(output.contains(".clone()"), 
            "L-04: Array access should have .clone(): {}", output);
    }
    
    /// Integration test: Complete function with all patterns
    #[test]
    fn test_integration_complete_function() {
        let input = r#"fn process() -> i32 {
    mut total = 0
    mut i = 0
    while i < 10 {
        total = total + 1
        i = i + 1
    }
    total
}"#;
        let output = parse_rusts(input);
        
        // L-01: mut declarations become let mut
        assert!(output.contains("let mut total"), 
            "Integration: 'mut total' must become 'let mut total': {}", output);
        assert!(output.contains("let mut i"), 
            "Integration: 'mut i' must become 'let mut i': {}", output);
        
        // L-03: Reassignments don't create new bindings
        let let_total_count = output.matches("let mut total").count() + output.matches("let total").count();
        assert_eq!(let_total_count, 1, 
            "Integration: Should have exactly 1 'let' for total: {}", output);
        
        // L-05: Balanced braces
        let open = output.matches('{').count();
        let close = output.matches('}').count();
        assert_eq!(open, close, 
            "Integration: Braces must be balanced: {}", output);
    }
    
    /// Test multi-line pub use import block transformation
    /// Items should have commas, closing brace should have semicolon
    #[test]
    fn test_multiline_use_import() {
        let input = r#"pub use quorum_da::{
    QuorumDA
    QuorumError
    ValidatorInfo
}"#;
        let output = parse_rusts(input);
        
        // Items should have commas
        assert!(output.contains("QuorumDA,"), 
            "Multi-line use: QuorumDA should have comma: {}", output);
        assert!(output.contains("QuorumError,"), 
            "Multi-line use: QuorumError should have comma: {}", output);
        assert!(output.contains("ValidatorInfo,"), 
            "Multi-line use: ValidatorInfo should have comma: {}", output);
        
        // Closing brace should have semicolon
        assert!(output.contains("};"), 
            "Multi-line use: closing brace should have semicolon: {}", output);
        
        // Opening should NOT have semicolon after {
        assert!(!output.contains("{;"), 
            "Multi-line use: opening should NOT have semicolon after brace: {}", output);
    }
    
    /// Test single-line use import is unchanged
    #[test]
    fn test_singleline_use_import() {
        let input = "use std::collections::{HashMap, HashSet}";
        let output = parse_rusts(input);
        
        // Single-line use should have semicolon at end
        assert!(output.contains("};") || output.ends_with(";"), 
            "Single-line use should have semicolon: {}", output);
    }
    
    /// Test simple use import
    #[test]
    fn test_simple_use_import() {
        let input = "use std::io::Result";
        let output = parse_rusts(input);
        
        // Simple use should have semicolon
        assert!(output.contains("Result;") || output.trim().ends_with(";"), 
            "Simple use should have semicolon: {}", output);
    }
}