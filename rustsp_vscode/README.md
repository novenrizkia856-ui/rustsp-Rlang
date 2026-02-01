# RustS+ Language Support for VS Code

**Full language support for RustS+ — the programming language with effect honesty.**

> *Rust prevents memory bugs. RustS+ prevents logic bugs.*

## Features

### Syntax Highlighting

Comprehensive syntax highlighting that understands RustS+ specific syntax:

- **Effects System** — `effects(io, alloc, panic, read x, write x)` highlighted distinctly
- **Effect Statements** — `effect write(x)` standalone statements
- **Clean Syntax** — `fn name(param Type) ReturnType` without `:` and `->`
- **Match Arms** — `Pattern { body }` without `=>`
- **Modifiers** — `mut`, `outer`, `pub` keywords
- **I/O Builtins** — `println()`, `print()` without `!`
- **Rust Macros** — `vec!`, `format!`, `assert!` with `!`
- **Generics** — `fn identity[T](x T) T` with `[]`
- **Struct/Enum Fields** — Field names highlighted in definitions
- **Attributes** — `#[derive(Clone, Debug)]` with derive trait recognition
- **Format Strings** — `{}` placeholders highlighted inside strings
- **All Number Formats** — Decimal, hex (`0xFF`), binary (`0b1010`), octal (`0o77`), float
- **Lifetimes** — `'a`, `'static`
- **Documentation Comments** — `///` highlighted differently from `//`
- **SCREAMING_CASE Constants** — Uppercase constants highlighted

### IntelliSense

- **Auto-completion** for keywords, types, effects, and built-in functions
- **Context-aware** effect completion inside `effects(...)` clauses
- **Snippet templates** for common patterns

### Hover Information

Hover over RustS+ keywords and effect types for documentation:
- **Effect types** — Shows description and detected patterns
- **Keywords** — Shows RustS+ specific behavior and error codes

### Document Symbols (Outline)

Navigate your code with the Outline view:
- Functions (with effect annotations shown)
- Structs, Enums, Traits
- Impl blocks
- Modules

### Code Snippets

| Prefix | Description |
|--------|-------------|
| `main` | Main function with effects(io) |
| `fne` | Function with effects |
| `fnp` | Pure function (no effects) |
| `fng` | Generic function |
| `fn1` | Single-line function expression |
| `struct` | Struct definition |
| `enum` | Enum definition |
| `match` | Match expression (RustS+ style) |
| `ife` | If-else expression |
| `mut` | Mutable variable |
| `outer` | Outer scope mutation |
| `effects` | Effects clause |
| `eio` | Effects(io) clause |
| `ew` | Effects(write x) clause |
| `pl` | println() |
| `plf` | println() with format |
| `impl` | Implementation block |
| `trait` | Trait definition |
| `test` | Test function |
| `wallet` | Complete wallet example |

## Installation

### From Source

```bash
# Clone the extension
git clone <repo-url>
cd rusts-plus-extension

# Install dependencies
npm install

# Build
npm run build

# Package
npx @vscode/vsce package

# Install the generated .vsix file
code --install-extension rusts-plus-0.2.0.vsix
```

### Development

```bash
# Watch mode for development
npm run watch

# Then press F5 in VS Code to launch Extension Development Host
```

## RustS+ Syntax Quick Reference

```rust
// Variables (no 'let' keyword)
x = 10
mut counter = 0
outer x = x + 1

// Functions
fn add(a i32, b i32) i32 { a + b }
fn greet(name String) effects(io) { println("Hello, {}", name) }
fn identity[T](x T) T { x }

// Structs (no colon in fields)
struct Point { x i32, y i32 }

// Enums
enum Msg { Quit, Move { x i32, y i32 } }

// Match (no => in arms)
match msg {
    Msg::Quit { println("quit") }
    Msg::Move { x, y } { println("{} {}", x, y) }
}

// Effects
effects(io)           // I/O operations
effects(alloc)        // Heap allocation
effects(panic)        // May panic
effects(read x)       // Reads parameter x
effects(write x)      // Mutates parameter x
```

## Requirements

- VS Code 1.85.0 or later

## License

MIT
