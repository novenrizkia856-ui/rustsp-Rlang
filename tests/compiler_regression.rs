use rustsp::parse_rusts;

#[test]
fn case_a_method_chain_atomicity_expect() {
    let src = include_str!("../rustsp/tests/compiler_regression/test_chain_a1.rss");
    let out = parse_rusts(src);
    assert!(out.contains("get_tuple().expect(\"fail\")"));
    assert!(!out.contains("\n.expect("));
    assert!(!out.contains("compile_error!("));
}

#[test]
fn case_a_method_chain_atomicity_chain() {
    let src = "fn main() {\n    let x = foo().bar().baz();\n}";
    let out = parse_rusts(src);
    assert!(out.contains("foo().bar().baz()"));
    assert!(!out.contains("\n.bar("));
}

#[test]
fn case_b_non_result_tuple_ok() {
    let src = include_str!("../rustsp/tests/compiler_regression/test_non_result_tuple_b.rss");
    let out = parse_rusts(src);
    assert!(out.contains("let (a, b) = foo();"));
    assert!(!out.contains("compile_error!("));
}

#[test]
fn case_c_result_tuple_without_handling_emits_compile_error() {
    let src = include_str!("../rustsp/tests/compiler_regression/test_result_tuple_no_handling_c.rss");
    let out = parse_rusts(src);
    assert!(out.contains("compile_error!(\"RustS+ semantic error: tuple destructuring from Result requires .expect(...), .unwrap(...), or ?\")"));
}

#[test]
fn case_d_result_tuple_with_expect_ok() {
    let src = include_str!("../rustsp/tests/compiler_regression/test_result_tuple_expect_d.rss");
    let out = parse_rusts(src);
    assert!(out.contains("let (a, b) = foo().expect(\"fail\");"));
    assert!(!out.contains("compile_error!("));
}

#[test]
fn case_e_slice_to_array_uses_try_into_without_clone() {
    let src = include_str!("../rustsp/tests/compiler_regression/test_slice_to_array_e.rss");
    let out = parse_rusts(src);
    assert!(out.contains("try_into().expect(\"RustS+: slice length mismatch during array conversion\")"));
    assert!(!out.contains("[0..32].clone()"));
}

#[test]
fn case_a_method_chain_multiline_merges() {
    let src = "fn foo() -> Result<(u8, u8), String> {\n    Ok((1,2))\n}\n\nfn main() {\n    let (a, b) = foo()\n        .expect(\"fail\");\n}";
    let out = parse_rusts(src);
    assert!(out.contains("let (a, b) = foo().expect(\"fail\");"));
    assert!(!out.contains("
.expect("));
}
