# RustS+ (RustSPlus)

**The Programming Language with Effect Honesty**
*Rust prevents memory bugs. RustS+ prevents logic bugs.*


---

## 📋 Table of Contents

- [What is RustS+?](#-what-is-rusts)
- [Philosophy](#-philosophy)
- [Quick Start](#-quick-start)
- [Architecture Overview](#-architecture-overview)
- [Compilation Pipeline](#-compilation-pipeline)
- [The Anti-Fail Logic System](#-the-anti-fail-logic-system)
- [Effect Ownership Model](#-effect-ownership-model)
- [Syntax Reference](#-syntax-reference)
- [Module Structure](#-module-structure)
- [Error System](#-error-system)
- [Cargo Integration](#-cargo-integration)
- [Technical Deep Dive](#-technical-deep-dive)
- [Contributing](#-contributing)

---

## What is RustS+?

RustS+ is a superset of Rust that adds a layer of logic safety on top of Rust's memory safety. RustS+ introduces the concept of effect ownership — a system that forces programmers to be honest about what their code does.

```
┌─────────────────────────────────────────────────────────┐
│                    RustS+ Layer                         │
│   ┌─────────────────────────────────────────────────┐   │
│   │  Effect Ownership • Logic Safety • Intent       │   │
│   │  Honesty • Anti-Fail Logic • Explicit State     │   │
│   └─────────────────────────────────────────────────┘   │
│                         ↓                               │
│   ┌─────────────────────────────────────────────────┐   │
│   │               Rust Layer                        │   │
│   │  Memory Safety • Type Safety • Ownership        │   │
│   │  Borrowing • Lifetimes • Zero-Cost Abstraction  │   │
│   └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

### Key Features

| Feature | Description |
|---------|-------------|
| **Effect Ownership** | Functions must declare side effects (`io`, `write`, `alloc`, `panic`) |
| **Anti-Fail Logic** | 6 logic rules + 6 effect rules enforced at compile time |
| **Honest Code** | No hidden mutations, no surprise side effects |
| **Clean Syntax** | Streamlined syntax without sacrificing safety |
| **Rust Backend** | Compiles to native Rust, then to machine code |

---

## Philosophy

### The Problem RustS+ Solves

Rust prevents **memory bugs** — use-after-free, double-free, data races. But Rust doesn't prevent **logic bugs**:

```rust
// Rust allows this - looks pure but has hidden effects
fn calculate_price(item: &Item) -> f64 {
    println!("Calculating..."); // Hidden I/O!
    log_to_file(&item);         // Hidden I/O!
    global_counter += 1;        // Hidden mutation!
    item.price * 1.1
}
```

### The RustS+ Solution

RustS+ forces honesty:

```rust
// RustS+ - effects must be declared
fn calculate_price(item &Item) effects(io) f64 {
    println("Calculating...")   // OK - io declared
    item.price * 1.1
}

// Pure function - NO effects allowed
fn pure_calculate(item &Item) f64 {
    println("...")  // ERROR! Undeclared effect
    item.price * 1.1
}
```

### Core Principles

1. Effect Honesty: If a function performs an effect, it MUST have a declaration.
2. Intent Clarity: There is no ambiguity about what the code does.
3. Explicit State: All state changes must be explicit.
4. No Hidden Mutations: Assignment = new declaration, not silent mutation.
5. Compile-Time Enforcement: All rules are enforced before runtime.

---

## Quick Start

### Installation

```bash
# Clone repository
git https://github.com/novenrizkia856-ui/rustsp-Rlang
cd rustsp-Rlang-main

# Build compiler
cargo build --release

# Install to PATH
cp target/release/rustsp ~/.cargo/bin/
cp target/release/cargo-rustsp ~/.cargo/bin/
```

### Hello World

create a file `hello.rss`:

```rust
fn main() effects(io) {
    println("Hello, RustS+!")
}
```

Compile and run:

```bash
rustsp hello.rss -o hello
./hello
```

### Your First Program

```rust
// wallet.rss

struct Wallet {
    id u32
    balance i64
}

enum Transaction {
    Deposit { amount i64 }
    Withdraw { amount i64 }
}

// Pure function - no effects
fn apply_tx(w Wallet, tx Transaction) Wallet {
    match tx {
        Transaction::Deposit { amount } {
            Wallet {
                id = w.id
                balance = w.balance + amount
            }
        }
        Transaction::Withdraw { amount } {
            Wallet {
                id = w.id
                balance = w.balance - amount
            }
        }
    }
}

// Effectful function - io declared
fn print_balance(w &Wallet) effects(io) {
    println("Balance: {}", w.balance)
}

fn main() effects(io) {
    wallet = Wallet { id = 1, balance = 100 }
    tx = Transaction::Deposit { amount = 50 }
    
    new_wallet = apply_tx(wallet, tx)
    print_balance(&new_wallet)
}
```

---

## 🏗️ Architecture Overview

```
┌──────────────────────────────────────────────────────────────────────┐
│                        RustS+ Compiler                               │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐              │
│  │   main.rs   │───▶│   lib.rs    │───▶│  Output.rs  │───▶ rustc    │
│  │  (Driver)   │    │ (Lowering)  │    │  (Valid     │              │
│  └─────────────┘    └─────────────┘    │   Rust)     │              │
│         │                  │           └─────────────┘              │
│         ▼                  ▼                                        │
│  ┌─────────────────────────────────────────┐                        │
│  │         Anti-Fail Logic System          │                        │
│  │  ┌─────────────┐  ┌─────────────────┐  │                        │
│  │  │ Logic Rules │  │  Effect System  │  │                        │
│  │  │  (L-01~06)  │  │   (E-01~06)     │  │                        │
│  │  └─────────────┘  └─────────────────┘  │                        │
│  └─────────────────────────────────────────┘                        │
│         │                                                           │
│         ▼                                                           │
│  ┌─────────────────────────────────────────┐                        │
│  │            Supporting Modules           │                        │
│  │  ┌──────────┐ ┌──────────┐ ┌─────────┐ │                        │
│  │  │ function │ │  scope   │ │variable │ │                        │
│  │  └──────────┘ └──────────┘ └─────────┘ │                        │
│  │  ┌──────────┐ ┌──────────┐ ┌─────────┐ │                        │
│  │  │struct_def│ │ enum_def │ │ctrl_flow│ │                        │
│  │  └──────────┘ └──────────┘ └─────────┘ │                        │
│  └─────────────────────────────────────────┘                        │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### Module Responsibilities

| Module | File | Responsibility |
|--------|------|----------------|
| **Driver** | `main.rs` | CLI, pipeline orchestration, rustc invocation |
| **Lowering** | `lib.rs` | RustS+ → Rust syntax transformation |
| **Anti-Fail Logic** | `anti_fail_logic.rs` | Logic rules, effect system, validation |
| **Function** | `function.rs` | Function parsing, signature transformation |
| **Scope** | `scope.rs` | Scope stack, variable lookup, shadowing |
| **Variable** | `variable.rs` | Variable tracking, mutation detection |
| **Control Flow** | `control_flow.rs` | Match/if transformation, arm handling |
| **Struct Def** | `struct_def.rs` | Struct definition and instantiation |
| **Enum Def** | `enum_def.rs` | Enum definition and pattern matching |
| **Semantic Check** | `semantic_check.rs` | Pre-lowering semantic validation |
| **Error Msg** | `error_msg.rs` | Error codes, formatting, Rust error mapping |
| **Rust Sanity** | `rust_sanity.rs` | Output validation before rustc |

---

## Formal IR Pipeline

RustS+ isn't just a "language with new syntax" — it's a formal system for ensuring the correctness of program meaning. Its architecture is built on a series of formal Intermediate Representations (IRs):

```
┌─────────────────────────────────────────────────────────────────────┐
│                    FORMAL IR PIPELINE                                │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  SOURCE (.rss)                                                       │
│       │                                                              │
│       ▼                                                              │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  AST (Abstract Syntax Tree)                                  │    │
│  │    → Structure: expressions, statements, items               │    │
│  │    → NO semantic meaning yet                                 │    │
│  └─────────────────────────────────────────────────────────────┘    │
│       │                                                              │
│       ▼                                                              │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  HIR (High-level IR)                                         │    │
│  │    → Resolved bindings (names → binding IDs)                 │    │
│  │    → Scope information                                       │    │
│  │    → Mutability tracking                                     │    │
│  │    → `outer` keyword resolution                              │    │
│  └─────────────────────────────────────────────────────────────┘    │
│       │                                                              │
│       ▼                                                              │
│  ┌─────────────────────────────────────────────────────────────┐    │
│  │  EIR (Effect IR)                                             │    │
│  │    → Effect inference (structural, not heuristic)            │    │
│  │    → Effect propagation checking                             │    │
│  │    → Effect ownership validation                             │    │
│  │    → Effect Graph construction                               │    │
│  └─────────────────────────────────────────────────────────────┘    │
│       │                                                              │
│       ▼                                                              │
│  OUTPUT (.rs) ──▶ rustc ──▶ Binary                                  │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### Why Formal IR?

With this architecture, RustS+ becomes a semantic compiler that understands what a program does formally, not just a text transformer:

| Approach | Problem |
|----------|---------|
| Regex/Text-based | Doesn't understand context, prone to errors |
| AST-only | Doesn't understand scope and binding |
| HIR + EIR | Understands meaning and effect formally |
---

## Two-Layer Type System

RustS+ has a **two-layer Type System**:

```
┌───────────────────────────────────────────────────────────────┐
│  LAYER 2: EFFECT CAPABILITY SYSTEM                            │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  read(x)  │  write(x)  │  io  │  alloc  │  panic        │  │
│  │                                                         │  │
│  │  "Every value has not only a data type,                 │  │
│  │ but also a RIGHT to reality"                            │  │
│  └─────────────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────────────┤
│  LAYER 1: RUST TYPE SYSTEM                                    │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  i32, String, struct, enum, borrow, generics, lifetimes │  │
│  └─────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

### Effect as a Linear Resource

The `write(x)` capability is treated as a linear resource** — just like `&mut T` in Rust:

- **Must not be duplicated** — only one party can have `write(x)` at a time
- **Must be propagated** — if a function has write capability, the caller must declare or propagate it
- **Exclusive ownership** — two functions cannot write to the same state without coordination

```rust
// write(acc) is the "exclusive write token" for acc
fn deposit(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance + amount  // OK - has write token
    acc
}

fn withdraw(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance - amount  // OK - has write token
    acc
}

// ERROR: Two write tokens for acc on the same execution path
// will be detected as RSPL315: Effect ownership violation
```

### Function Type Signature

Every function in RustS+ is formally typed:

```
(parameter types) → return type + capability set
```

Example:
```rust
fn transfer(from Account, to Account, amount i64) 
    effects(write from, write to) 
    (Account, Account)
    
// Formal signature type:
// (Account, Account, i64) → (Account, Account) + {write(from), write(to)}
```

---

## Compilation Pipeline

RustS+ uses a **4-stage compilation pipeline**:
```
┌─────────────────────────────────────────────────────────────────────┐
│                    STAGE 0: Analysis                                │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│  • Parse all function signatures                                    │
│  • Extract effect declarations: effects(io, write x, ...)          │
│  • Build function table with effect contracts                       │
│  • Build effect dependency graph for cross-function analysis        │
│                                                                     │
│  Input:  fn foo(x T) effects(io) R { ... }                         │
│  Output: FunctionInfo { name: "foo", effects: [io], ... }          │
├─────────────────────────────────────────────────────────────────────┤
│                    STAGE 1: Anti-Fail Logic                         │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│                                                                     │
│  LOGIC CHECKS:                                                      │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ Logic-01: Expression completeness (if/match branches)       │   │
│  │ Logic-02: Ambiguous shadowing detection                     │   │
│  │ Logic-03: Illegal statements in expression context          │   │
│  │ Logic-04: Implicit mutation detection                       │   │
│  │ Logic-05: Unclear intent patterns                           │   │
│  │ Logic-06: Same-scope reassignment without mut               │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  EFFECT CHECKS:                                                     │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │ Effect-01: Undeclared effect validation                     │   │
│  │ Effect-02: Effect leak detection (closures)                 │   │
│  │ Effect-03: Pure calling effectful detection                 │   │
│  │ Effect-04: Cross-function effect propagation                │   │
│  │ Effect-05: Effect scope validation                          │   │
│  │ Effect-06: Effect ownership/conflict detection              │   │
│  └─────────────────────────────────────────────────────────────┘   │
│                                                                     │
│  ⚠️  ANY VIOLATION = COMPILATION STOPS                             │
│  ⚠️  NO RUST CODE GENERATED                                        │
├─────────────────────────────────────────────────────────────────────┤
│                    STAGE 2: Lowering                                │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│                                                                     │
│  SYNTAX TRANSFORMATIONS:                                            │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ L-01: fn foo(x T) R → fn foo(x: T) -> R                    │    │
│  │ L-02: Match arms { body } → => { body },                   │    │
│  │ L-03: x = 10 → let x = 10;                                 │    │
│  │ L-04: mut x = 10 → let mut x = 10;                         │    │
│  │ L-05: effects(...) → (stripped entirely)                   │    │
│  │ L-06: [T] param → &[T] param                               │    │
│  │ L-07: effect write(x) → (skipped)                          │    │
│  │ L-08: println(...) → println!(...)                         │    │
│  │ L-09: Match arm parens fix                                 │    │
│  │ L-10: Call-site borrow insertion                           │    │
│  │ L-11: Slice index clone insertion                          │    │
│  │ L-12: Auto #[derive(Clone)] injection                      │    │
│  └────────────────────────────────────────────────────────────┘    │
│                                                                     │
│  RUST SANITY GATE:                                                  │
│  ┌────────────────────────────────────────────────────────────┐    │
│  │ • Balanced delimiters check                                │    │
│  │ • Illegal token detection                                  │    │
│  │ • Effect annotation leakage check                          │    │
│  │ • Unclosed string detection                                │    │
│  └────────────────────────────────────────────────────────────┘    │
├─────────────────────────────────────────────────────────────────────┤
│                    STAGE 3: Rust Compilation                        │
│  ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━ │
│  • Invoke rustc on generated .rs file                              │
│  • Map rustc errors back to RustS+ source locations                │
│  • Output binary or library                                         │
└─────────────────────────────────────────────────────────────────────┘
```

### Pipeline Guarantees

| Guarantee | Description |
|-----------|-------------|
| **Honest Code Only** | If code passes Stage 1, it honestly declares all effects |
| **Valid Rust Output** | If Stage 2 completes, output is syntactically valid Rust |
| **Effect-Free Output** | Effects are compile-time only, never in generated Rust |
| **Deterministic** | Same input always produces same output |

---

## The Anti-Fail Logic System

Anti-Fail Logic is the heart of RustS+. This system consists of 6 Logic Rules and 6 Effect Rules.

### Logic Rules

#### Logic-01: Expression Completeness

An `if`/`match` used as a value MUST contain all branches.

```rust
// ❌ INVALID - missing else
result = if x > 0 { "positive" }

// ✅ VALID
result = if x > 0 { "positive" } else { "negative" }
```

**Error Code:** `RSPL060`

#### Logic-02: Ambiguous Shadowing

Assignment to an outer scope variable without the `outer` keyword will ERROR.

```rust
// ❌ INVALID - ambiguous
x = 10
{
    x = 20  // Creates new variable or modifies outer?
}

// ✅ VALID - explicit outer mutation
x = 10
{
    outer x = 20  // Clearly modifies outer
}
```

**Error Code:** `RSPL081`

#### Logic-03: Illegal Statement in Expression

`let` statements must not appear in the expression context.

```rust
// ❌ INVALID
result = {
    let temp = 10;  // Statement in expression!
    temp
}

// ✅ VALID
result = {
    temp = 10  // RustS+ assignment (becomes let)
    temp
}
```

**Error Code:** `RSPL041`

#### Logic-04: Implicit Mutation

Struct field mutations must be trackable.

#### Logic-05: Unclear Intent

Confusing patterns such as empty blocks `{}` will be flagged.

**Error Code:** `RSPL001`

#### Logic-06: Same-Scope Reassignment

Reassignments within the same scope MUST use `mut`.
```rust
// ❌ INVALID
x = 10
x = 20  // Reassignment without mut!

// ✅ VALID
mut x = 10
x = 20  // OK - declared as mut
```

**Error Code:** `RSPL071`

### Effect Rules

#### Effect-01: Undeclared Effect

If the function performs an effect, it MUST have a declaration.

```rust
// ❌ INVALID
fn greet() {
    println("Hello")  // io effect not declared!
}

// ✅ VALID
fn greet() effects(io) {
    println("Hello")
}
```

**Error Code:** `RSPL300`

#### Effect-02: Effect Leak

Effects must not leak into the closure without propagation.

#### Effect-03: Pure Calling Effective

Pure functions MUST NOT call effectful functions.

```rust
// ❌ INVALID
fn effectful() effects(io) { println("!") }
fn pure_func() {
    effectful()  // Pure calling effectful!
}

// ✅ VALID
fn effectful() effects(io) { println("!") }
fn caller() effects(io) {
    effectful()  // Effect propagated
}
```

**Error Code:** `RSPL302`

#### Effect-04: Missing Propagation

The effect of the callee MUST be propagated to the caller.

```rust
// ❌ INVALID
fn inner() effects(io) { println("inner") }
fn outer() {
    inner()  // Missing io propagation!
}

// ✅ VALID
fn inner() effects(io) { println("inner") }
fn outer() effects(io) {
    inner()
}
```

**Error Code:** `RSPL301`

#### Effect-05: Effect Scope Violation

The effect must be executed within a valid scope.

#### Effect-06: Concurrent Effect Conflict

Two effect sources cannot write the same state.

---

## Effect Ownership Model

### Concept: Borrow Checker for Program Meaning

Just as Rust has a borrow checker for memory, RustS+ has an effect checker for program meaning.
```
┌─────────────────────────────────────────────────────────────┐
│                    OWNERSHIP PARALLEL                       │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│   RUST (Memory)              RUSTS+ (Effects)               │
│   ─────────────              ───────────────                │
│   • One owner per value      • One source per effect        │
│   • Borrow to share          • Propagate to share           │
│   • Mut exclusive            • Write exclusive              │
│   • Compile-time checked     • Compile-time checked         │
│                                                             │
└─────────────────────────────────────────────────────────────┘
```

### Effect Types

| Effect | Syntax | Description | Propagatable | Examples |
|--------|--------|-------------|--------------|----------|
| `io` | `effects(io)` | I/O operations | ✅ Yes | `println!`, `File::open`, `TcpStream::connect`, `env::var` |
| `alloc` | `effects(alloc)` | Heap memory allocation | ✅ Yes | `Vec::new()`, `Box::new()`, `String::from()`, `format!` |
| `panic` | `effects(panic)` | May panic at runtime | ✅ Yes | `.unwrap()`, `.expect()`, `panic!`, `assert!` |
| `read(x)` | `effects(read x)` | Read from parameter x | ❌ No | `x.field`, passing `x` to function |
| `write(x)` | `effects(write x)` | Write/mutate parameter x | ❌ No | `x.field = value`, `*x = value` |

#### Important Notes on Effect Detection

**What IS detected as `alloc`:**
- Explicit heap constructors: `Vec::new()`, `Box::new()`, `String::from()`, `HashMap::new()`
- Allocating macros: `vec!`, `format!`
- Methods that create new heap objects: `.to_string()`, `.to_owned()`, `.to_vec()`

**What is NOT detected as `alloc` (by design):**
- `.clone()` — Because cloning Copy types (i32, bool, etc.) doesn't allocate
- `.collect()` — Because output type varies; may not allocate
- Struct literals — `Point { x: 0, y: 0 }` is stack-allocated, not heap
- Tuple literals — `(1, 2, 3)` is stack-allocated
- Fixed arrays — `[1, 2, 3]` is stack-allocated

**What IS detected as `io`:**
- Console: `println!`, `print!`, `stdin()`, `stdout()`
- File: `File::open`, `fs::read`, `fs::write`, `BufReader`
- Network: `TcpStream::connect`, `UdpSocket::bind`, `.send()`, `.recv()`
- Environment: `env::var`, `env::args`, `env::current_dir`
- Process: `Command::new`, `.spawn()`, `.output()`

### Effect Lifecycle

```
┌──────────────────────────────────────────────────────────────────────┐
│                      Effect Lifecycle                                │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  1. DECLARATION                                                      │
│     fn process(data Data) effects(write data, io) Result { ... }    │
│                            └──────────────────┘                      │
│                            Effect contract declared                  │
│                                                                      │
│  2. DETECTION                                                        │
│     Compiler scans function body for effect operations:              │
│     • println! → io                                                  │
│     • data.field = x → write(data)                                   │
│     • Vec::new() → alloc                                             │
│     • unwrap() → panic                                               │
│                                                                      │
│  3. VALIDATION                                                       │
│     Detected effects ⊆ Declared effects                              │
│     If not → RSPL300 error                                           │
│                                                                      │
│  4. PROPAGATION                                                      │
│     If function A calls function B with effects(io):                 │
│     A must also declare effects(io)                                  │
│     If not → RSPL301 error                                           │
│                                                                      │
│  5. STRIPPING                                                        │
│     effects(...) clause removed during lowering                      │
│     Effect is compile-time only contract                             │
│                                                                      │
└──────────────────────────────────────────────────────────────────────┘
```

### Effect Inference Algorithm

RustS+ uses the Effect Inference Algorithm that runs on top of the HIR. Each expression and statement generates an effect trace that is computed structurally, not text/regex-based:

| Expression/Statement | Inferred Effect | Reasoning |
|---------------------|-------------------|----------|
| `42`, `true` | ∅ (none) | Numeric/boolean literals produce no effect |
| `"hello"` | ∅ (none) | String literals in code are static, not heap |
| `x` (read param) | `read(x)` | Reading a parameter produces a read effect |
| `x` (read local) | ∅ (none) | Reading a local variable has no effect |
| `Point { x: 0, y: 0 }` | ∅ (none) | Struct literals are stack-allocated |
| `(1, 2, 3)` | ∅ (none) | Tuple literals are stack-allocated |
| `[1, 2, 3]` | ∅ (none) | Fixed array literals are stack-allocated |
| `w.field` | `read(w)` if param | Field access = read owner object |
| `w.field = 3` | `write(w)` | Field mutation = owner mutation |
| `w = new_w` | ∅ (none) | Rebinding ≠ content mutation |
| `println!(...)` | `io` | I/O operations |
| `Vec::new()` | `alloc` | Explicit heap allocation |
| `Box::new(x)` | `alloc` | Explicit heap allocation |
| `x.clone()` | ∅ (none)* | *May or may not allocate depending on type |
| `.unwrap()` | `panic` | May panic |
| `f(args...)` | `effects(f) ∪ effects(args)` | Combined caller + callee |
| `if c { a } else { b }` | `effects(c) ∪ effects(a) ∪ effects(b)` | Union of all branches |

**Key Insights:**

1. **Field Mutation = Owner Mutation:** A mutation to a **field** (`w.x = 3`) is treated as a mutation to the **owner object** (`write(w)`). This is because changing the field changes the *state* of the entire object.

2. **Stack vs Heap:** Struct, tuple, and array literals are **stack-allocated** by default in Rust. Only explicit heap constructors (`Vec::new`, `Box::new`, etc.) produce `alloc` effects.

3. **Conservative on `.clone()`:** The compiler does NOT automatically flag `.clone()` as alloc because cloning Copy types (i32, u64, bool) doesn't allocate. For strict tracking, declare `effects(alloc)` explicitly when cloning heap types.

### Effect Dependency Graph

RustS+ builds a **dependency graph** for cross-function effect analysis:

```
           ┌──────────────────┐
           │      main()      │
           │  effects(io)     │
           └────────┬─────────┘
                    │ calls
          ┌─────────┴─────────┐
          ▼                   ▼
   ┌──────────────┐    ┌──────────────┐
   │  process()   │    │   log()      │
   │ effects(io,  │    │ effects(io)  │
   │  write data) │    └──────────────┘
   └──────┬───────┘
          │ calls
          ▼
   ┌──────────────┐
   │  validate()  │
   │   (pure)     │
   └──────────────┘
```

### Stack vs Heap: Why It Matters

RustS+ distinguishes between **stack-allocated** and **heap-allocated** data structures. This is critical for accurate effect detection:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    MEMORY ALLOCATION IN RUST                         │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  STACK (No alloc effect)              HEAP (alloc effect)           │
│  ─────────────────────                ──────────────────            │
│  • Fixed-size, known at compile       • Dynamic size                │
│  • Fast allocation (just move SP)     • Slower allocation           │
│  • Automatic cleanup                  • Needs explicit management   │
│                                                                      │
│  Examples:                            Examples:                      │
│  • let x = 42;                        • let v = Vec::new();         │
│  • let p = Point { x: 0, y: 0 };      • let s = String::from("x"); │
│  • let t = (1, 2, 3);                 • let b = Box::new(42);       │
│  • let a = [1, 2, 3];                 • let v = vec![1, 2, 3];      │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

#### Why RustS+ Doesn't Flag Struct Literals as `alloc`

```rust
// ✅ This is PURE - no alloc effect needed
fn create_point() Point {
    Point { x: 0, y: 0 }  // Stack-allocated!
}

// ✅ This REQUIRES alloc effect
fn create_points() Vec[Point] effects(alloc) {
    vec![
        Point { x: 0, y: 0 },  // Point is stack, but Vec is heap
        Point { x: 1, y: 1 },
    ]
}
```

#### The `.clone()` Dilemma

```rust
// ✅ No alloc - i32 is Copy, clone just copies bits
fn double(x i32) i32 {
    x.clone() + x  // No heap allocation!
}

// ⚠️ Alloc - String is heap type, clone allocates new heap memory
fn duplicate(s String) String effects(alloc) {
    s.clone()  // User declares alloc because they know String is heap
}
```

**Philosophy:** RustS+ trusts the programmer to know their types. Rather than producing false positives for every `.clone()`, it requires explicit declaration when the programmer knows heap allocation occurs.

---

## Syntax Reference

### Variables

```rust
// Declaration (immutable by default)
x = 10                    // → let x = 10;
name = "Alice"            // → let name = String::from("Alice");

// Mutable declaration
mut counter = 0           // → let mut counter = 0;
counter = counter + 1     // OK - counter is mut

// Outer mutation (across scopes)
x = 10
{
    outer x = x + 1       // Modifies outer x
}

// Type annotation (optional)
x i32 = 10                // → let x: i32 = 10;
```

### Functions

```rust
// Basic function
fn add(a i32, b i32) i32 {
    a + b
}
// → fn add(a: i32, b: i32) -> i32 { a + b }

// With effects
fn greet(name String) effects(io) {
    println("Hello, {}", name)
}

// Generic function
fn identity[T](x T) T {
    x
}
// → fn identity<T>(x: T) -> T { x }

// Single-line function
fn double(x i32) i32 = x * 2

// Multiple effects
fn process(data Data) effects(io, write data) Data {
    println("Processing...")
    data.processed = true
    data
}
```

### Structs

```rust
// Definition
struct Point {
    x i32
    y i32
}
// → #[derive(Clone)] struct Point { x: i32, y: i32, }

// Instantiation
p = Point { x = 10, y = 20 }
// → let p = Point { x: 10, y: 20 };

// Field access
println("{}", p.x)

// Update syntax
p2 = Point { x = 100, ..p }
```

### Enums

```rust
// Definition
enum Message {
    Quit
    Move { x i32, y i32 }
    Write(String)
    Color(i32, i32, i32)
}

// Instantiation
msg = Message::Move { x = 10, y = 20 }
text = Message::Write("hello")

// Pattern matching
match msg {
    Message::Quit {
        println("Quit")
    }
    Message::Move { x, y } {
        println("Move to {}, {}", x, y)
    }
    Message::Write(s) {
        println("Write: {}", s)
    }
    _ {
        println("Other")
    }
}
```

### Control Flow

```rust
// if expression (all branches required when used as value)
result = if x > 0 {
    "positive"
} else if x < 0 {
    "negative"
} else {
    "zero"
}

// match expression
grade = match score {
    90..=100 { "A" }
    80..=89 { "B" }
    70..=79 { "C" }
    _ { "F" }
}

// while loop
mut i = 0
while i < 10 {
    println("{}", i)
    i = i + 1
}

// for loop (Rust syntax)
for item in items.iter() {
    println("{}", item)
}
```

### Syntax Comparison Table

| Concept | RustS+ | Rust |
|---------|--------|------|
| Variable | `x = 10` | `let x = 10;` |
| Mutable | `mut x = 10` | `let mut x = 10;` |
| Function param | `x i32` | `x: i32` |
| Return type | `fn f() i32` | `fn f() -> i32` |
| Generics | `fn f[T](x T)` | `fn f<T>(x: T)` |
| Effects | `effects(io)` | *(none)* |
| Match arm | `Pattern { body }` | `Pattern => { body },` |
| String literal | `"hello"` | `String::from("hello")` |

---

## 📁 Module Structure

### File Organization

```
rustsp/
├── src/
│   ├── main.rs              # Compiler driver & CLI
│   ├── lib.rs               # Core lowering logic (2500+ lines)
│   ├── anti_fail_logic.rs   # Effect system & logic checks (2500+ lines)
│   ├── error_msg.rs         # Error codes & formatting (1400+ lines)
│   ├── function.rs          # Function parsing (1000+ lines)
│   ├── control_flow.rs      # Match/if transformation (900+ lines)
│   ├── scope.rs             # Scope management (700+ lines)
│   ├── semantic_check.rs    # Semantic validation (700+ lines)
│   ├── variable.rs          # Variable tracking (400+ lines)
│   ├── struct_def.rs        # Struct handling (200+ lines)
│   ├── enum_def.rs          # Enum handling (300+ lines)
│   └── rust_sanity.rs       # Output validation (600+ lines)
├── cargo-rustsp/
│   └── main.rs              # Cargo integration tool
├── Cargo.toml
├── README.md
└── GUIDE.md                 # Language specification
```

### Module Dependencies

```
                    ┌──────────────────┐
                    │     main.rs      │
                    └────────┬─────────┘
                             │
              ┌──────────────┼──────────────┐
              ▼              ▼              ▼
       ┌──────────┐   ┌──────────┐   ┌──────────┐
       │  lib.rs  │   │anti_fail │   │rust_sanity│
       │(lowering)│   │ _logic   │   │  (gate)  │
       └────┬─────┘   └────┬─────┘   └──────────┘
            │              │
    ┌───────┼───────┬──────┼───────┬───────────┐
    ▼       ▼       ▼      ▼       ▼           ▼
┌────────┐┌────┐┌──────┐┌──────┐┌──────┐┌──────────┐
│function││scope││struct││ enum ││ctrl_ ││error_msg │
│        ││     ││ _def ││ _def ││flow  ││          │
└────────┘└────┘└──────┘└──────┘└──────┘└──────────┘
```

### Key Data Structures

#### `FunctionInfo` (anti_fail_logic.rs)

```rust
pub struct FunctionInfo {
    pub name: String,
    pub parameters: Vec<(String, String)>,
    pub return_type: Option<String>,
    pub declared_effects: EffectSignature,
    pub detected_effects: EffectSignature,
    pub line_number: usize,
    pub calls: Vec<String>,
    pub body_lines: Vec<String>,
}
```

#### `EffectSignature` (anti_fail_logic.rs)

```rust
pub struct EffectSignature {
    pub effects: BTreeSet<Effect>,
    pub is_pure: bool,
}
```

#### `ScopeStack` (scope.rs)

```rust
pub struct ScopeStack {
    pub scopes: Vec<Scope>,
    pub mut_needed: HashMap<(String, usize), bool>,
    control_flow_depth: usize,
}
```

#### `FunctionRegistry` (function.rs)

```rust
pub struct FunctionRegistry {
    functions: HashMap<String, FunctionSignature>,
}
```

---

## ❌ Error System

### Error Code Hierarchy

```
RSPL Error Codes
├── RSPL001-019: Logic Errors
│   ├── RSPL001: Generic logic error
│   ├── RSPL002: Unreachable code
│   └── RSPL003: Infinite loop
├── RSPL020-039: Structure Errors
│   ├── RSPL020: Invalid function signature
│   ├── RSPL021: Invalid struct definition
│   └── RSPL022: Invalid enum definition
├── RSPL040-059: Expression Errors
│   ├── RSPL040: Expression as statement
│   ├── RSPL041: Statement as expression
│   └── RSPL042: Invalid assignment target
├── RSPL060-079: Control Flow Errors
│   ├── RSPL060: If missing else (value context)
│   ├── RSPL061: Match missing arms
│   └── RSPL071: Same-scope reassignment
├── RSPL080-099: Scope Errors
│   ├── RSPL080: Variable not found
│   ├── RSPL081: Ambiguous shadowing
│   └── RSPL082: Invalid outer target
├── RSPL100-119: Ownership Errors
│   ├── RSPL100: Move after borrow
│   └── RSPL103: Use after move
├── RSPL200-299: Rust Backend Errors
│   └── RSPL200-204: Mapped rustc errors
└── RSPL300-349: Effect Errors
    ├── RSPL300: Undeclared effect
    ├── RSPL301: Missing propagation
    ├── RSPL302: Pure calling effectful
    └── RSPL303-316: Other effect violations
```

### Error Message Format

```
error[RSPL071][scope]: reassignment to `x` without `mut` declaration
  --> main.rss:5:5
    |
5   |     x = x + 1
    |     ^^^^^^^^^

note:
  Logic-06 VIOLATION: Same-Scope Reassignment

  variable `x` was first assigned on line 3.
  reassigning without `mut` is not allowed in RustS+.

help:
  change original declaration to:

    mut x = ...
```

### Error Source Location Tracking

```rust
pub struct SourceLocation {
    pub file: String,
    pub line: usize,
    pub column: usize,
    pub source_line: String,
    pub highlight_start: usize,
    pub highlight_len: usize,
}
```

---

## Cargo Integration

### Apa itu cargo-rustsp?

`cargo-rustsp` is a build tool that integrates the RustS+ compiler with the Cargo ecosystem. With cargo-rustsp, you can use familiar Cargo workflows (`cargo build`, `cargo run`, etc.) for RustS+ projects.

```
┌─────────────────────────────────────────────────────────────────────┐
│                        cargo-rustsp v1.0.0                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                     │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐             │
│  │  .rss files │───▶│   rustsp    │───▶│  .rs files  │───▶ cargo   │
│  │  (RustS+)   │    │  compiler   │    │  (Rust)     │    build    │
│  └─────────────┘    └─────────────┘    └─────────────┘             │
│         ▲                                                           │
│         │                                                           │
│  ┌──────┴──────────────────────────────────────────────────────┐   │
│  │  Features:                                                   │   │
│  │  • Multi-module resolution (nested modules, mod.rss)         │   │
│  │  • Workspace support (multiple crates)                       │   │
│  │  • Incremental compilation (SHA-256 + Merkle tree caching)   │   │
│  │  • Mixed .rs/.rss projects                                   │   │
│  │  • Feature flags support                                     │   │
│  │  • Source-mapped error reporting                             │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Key Features

| Features | Description |
|-------|-----------|
| **Multi-Module** | Full support for nested modules (`mod foo;` resolves to `foo.rss` or `foo/mod.rss`) |
| **Workspace** | Build multiple crates in a single workspace |
| **Incremental** | SHA-256 content hashing + Merkle tree structure tracking — only recompiles what changed |
| **Smart Detection** | Detects renames, moves, additions, deletions without unnecessary recompilation |
| **Mixed Projects** | Combine `.rs` (pure Rust) and `.rss` (RustS+) in a single project |
| **Features** | Full `--features` support like regular cargo |
| **Error Mapping** | Error messages point to the location in the original `.rss` file |

### Installation

```bash
# Clone repository
git clone https://github.com/novenrizkia856-ui/rustsp-Rlang
cd rustsp-Rlang-main

# Build compiler and cargo-rustsp
cargo build --release

# Install ke PATH
cp target/release/rustsp ~/.cargo/bin/
cp target/release/cargo-rustsp ~/.cargo/bin/

# verification
cargo rustsp --version
# Output: cargo-rustsp x.x.x
```

### Commands

| Command | Description |
|---------|-------------|
| `cargo rustsp build` | Compile RustS+ project |
| `cargo rustsp run` | Build and run |
| `cargo rustsp test` | Run tests |
| `cargo rustsp check` | Check tanpa compile binary |
| `cargo rustsp bench` | Run benchmarks |
| `cargo rustsp doc` | Generate documentation |

### RustS+ Toolchain Options

| Option | Description |
|--------|-------------|
| `--rustsp-force` | Force recompile semua file .rss (ignore cache) |
| `--rustsp-quiet` | Suppress rustsp preprocessing output |
| `--rustsp-keep` | Jangan hapus deployed .rs files setelah cargo selesai |
| `--rustsp-clean` | Hapus leftover .rs files dari source tree |
| `--rustsp-reset` | Reset cache total — hapus `target/rustsp/` dan mulai dari awal |
| `--rustsp-status` | Lihat status cache: jumlah file, merkle root, ukuran cache |

### Options

```bash
cargo rustsp build [OPTIONS]

OPTIONS:
    -r, --release              Build in release mode
    -q, --quiet                Suppress output
    -f, --force                Force rebuild (ignore cache)
    -p, --package <SPEC>       Build specific package (workspace)
    -j, --jobs <N>             Number of parallel jobs
    -F, --features <FEATURES>  Features to activate
    --all-features             Activate all features
    --no-default-features      Disable default features
```

### Project Structures

#### Single File Project

```
my_project/
├── Cargo.toml
└── src/
    └── main.rss
```

#### Multi-Module Project

```
my_project/
├── Cargo.toml
└── src/
    ├── main.rss           # mod utils; mod parser;
    ├── utils.rss          # Flat module
    └── parser/
        ├── mod.rss        # pub mod lexer; pub mod tokens;
        ├── lexer.rss
        └── tokens.rss
```

#### Library + Binary

```
my_project/
├── Cargo.toml
└── src/
    ├── lib.rss            # Library entry point
    ├── main.rss           # Binary entry point  
    ├── core.rss           # Library module
    └── api.rss            # Library module
```

#### Mixed .rs/.rss Project

```
my_project/
├── Cargo.toml
└── src/
    ├── main.rss           # RustS+ dengan effects
    ├── pure_rust.rs       # Pure Rust (tanpa effects)
    └── with_effects.rss   # RustS+ dengan effects
```

#### Workspace

```
my_workspace/
├── Cargo.toml             # [workspace] members = ["core", "cli"]
├── core/
│   ├── Cargo.toml
│   └── src/
│       └── lib.rss
└── cli/
    ├── Cargo.toml
    └── src/
        └── main.rss
```

### Module Resolution

cargo-rustsp follows Rust's module resolution rules:

```
mod foo;  →  find in order:
             1. foo.rss      (RustS+ file)
             2. foo/mod.rss  (RustS+ directory module)
             3. foo.rs       (Rust file)
             4. foo/mod.rs   (Rust directory module)
```

Custom path with attribute:
```rust
#[path = "custom/location.rss"]
mod my_module;
```

### Build Process

```
cargo rustsp build
        │
        ▼
┌───────────────────────────────────────┐
│ 1. Scan .rss files                    │
│    - SHA-256 hash setiap file         │
│    - Build Merkle tree dari paths     │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 2. Load compile.json manifest         │
│    - Compare merkle root (struktur)   │
│    - Compare content hash (per file)  │
│    - Detect: new/mod/rename/move/del  │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 3. Compile HANYA file yang berubah    │
│    rustsp file.rss --emit-rs          │
│    Simpan hasil di target/rustsp/     │
│                                       │
│    ⚠️ ERROR? STOPS HERE               │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 4. Deploy cached .rs ke source dirs   │
│    (copy dari target/rustsp/ → src/)  │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 5. cargo build/run/test               │
│    (standard Rust compiler)           │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 6. Auto-cleanup .rs dari source tree  │
│    Update compile.json manifest       │
└───────────────────────────────────────┘
```

### Incremental Compilation (SHA-256 + Merkle Tree)

cargo-rustsp v1.0.0 uses a smart caching system that stores compilation results in `target/rustsp/` so that it doesn't need to be recompiled every time a command is run.

```
target/rustsp/
├── compile.json          # Manifest: SHA-256 hashes, Merkle root, file mappings
└── [mirrored source]     # Cached compiled .rs files
    └── src/
        ├── main.rs
        ├── lib.rs
        └── models/
            └── user.rs
```

**How ​​change detection works:**

| Change Type | Detection | Action |
|--------------------|---------|------|
| New file | Path not in manifest | Compile |
| Contents changed | SHA-256 hash mismatch | Recompile |
| File renamed | Same hash, different name, same dir | Update cache (skip compile) |
| File moved | Same hash, same name, different dir | Update cache (skip compile) |
| File deleted | Existing in manifest but missing from disk | Remove from cache |
| Unchanged | Path & hash match | Skip, use cache directly |

**Merkle trees** are used to quickly detect changes in project structure — if the root hashes are the same, there's no need for a per-file check. If they differ, the toolchain performs a diff to determine which files were changed, renamed, moved, or deleted.

**Example output:**

```
Preprocessing RustS+ files (incremental)...
      [NEW] src/models/order.rss
      [MOD] src/main.rss
   [RENAMED] src/customer.rss ← src/user.rss
      [DEL] src/old_module.rss
   Compiling src/models/order.rss
   Compiling src/main.rss
  Preprocessed 2 compiled, 5 cached, 1 renamed/moved, 1 removed
  Structure project layout changed (merkle root updated)
    Deployed 8/8 .rs file(s) to source tree
     Running cargo build
```

### Workspace Build

for workspace with multiple crates:

```bash
# build all members has a .rss files
cargo rustsp build

# Build specific package
cargo rustsp build -p core

# Build all packages (including pure Rust)
cargo rustsp build --workspace
```

### Feature Flags

```bash
# Enable specific features
cargo rustsp build --features="async,serde"

# Enable all features
cargo rustsp build --all-features

# Disable default features
cargo rustsp build --no-default-features --features="minimal"
```

### Troubleshooting

#### "Could not find Cargo.toml"
Make sure you are in the directory containing `Cargo.toml` or its subdirectories.

#### "No .rss files found"
cargo-rustsp will fallback to plain `cargo` if there is no `.rss` file.

#### "rustsp: command not found"
Make sure the `rustsp` compiler is in the PATH or in the same directory as `cargo-rustsp`.

#### Cache Issues
If the build feels stale:
```bash
# Lihat status cache
cargo rustsp --rustsp-status

# Force recompile all
cargo rustsp build --rustsp-force

# total reset from beginning
cargo rustsp --rustsp-reset
```

#### Module Not Found
Make sure the folder structure follows the convention:
- `mod foo;` → need `foo.rss` OR `foo/mod.rss`

## 🔬 Technical Deep Dive

### Lowering Implementation

Lowering is the process of transforming RustS+ syntax into valid Rust. The `lib.rs` file contains the main implementation.

#### Key Transformation Functions

```rust
// Function signature transformation
pub fn signature_to_rust(sig: &FunctionSignature) -> String {
    // fn foo(x i32) i32 → fn foo(x: i32) -> i32
}

// Assignment transformation  
pub fn parse_rusts_assignment(line: &str) -> Option<Assignment> {
    // x = 10 → let x = 10;
    // mut x = 10 → let mut x = 10;
}

// Match arm transformation
pub fn transform_arm_pattern(line: &str) -> String {
    // Pattern { → Pattern => {
}

// Effect stripping
pub fn strip_effects_clause(sig: &str) -> String {
    // fn f() effects(io) R → fn f() R
}
```

#### Lowering Rules (L-01 through L-12)

| Rule | From | To | Implementation |
|------|------|-----|----------------|
| L-01 | `fn f(x T) R` | `fn f(x: T) -> R` | `signature_to_rust()` |
| L-02 | `Pattern { body }` | `Pattern => { body },` | `transform_arm_pattern()` |
| L-03 | `x = 10` | `let x = 10;` | `parse_rusts_assignment()` |
| L-04 | `mut x = 10` | `let mut x = 10;` | `parse_rusts_assignment()` |
| L-05 | `effects(...)` | *(stripped)* | `strip_effects_clause()` |
| L-06 | `[T]` param | `&[T]` param | `transform_param_type()` |
| L-07 | `effect write(x)` | *(skipped)* | Line skip in parser |
| L-08 | `println(...)` | `println!(...)` | `transform_macro_calls()` |
| L-09 | Match parens | Fixed | `transform_arm_close_with_parens()` |
| L-10 | Call-site | `&arr` | `coerce_argument()` |
| L-11 | `arr[i]` | `arr[i].clone()` | `coerce_argument()` |
| L-12 | `struct S {}` | `#[derive(Clone)] struct S {}` | Auto-injection |

### Effect Detection Implementation

```rust
// Effect detection patterns - ACCURATE VERSION
// Note: Detection is conservative to avoid false positives

fn detect_io_effect(line: &str) -> bool {
    let io_patterns = [
        // Console I/O
        "println!", "print!", "eprintln!", "eprint!",
        "stdin()", "stdout()", "stderr()",
        // File I/O
        "File::", "fs::read", "fs::write", "fs::open",
        "BufReader::", "BufWriter::",
        // Network I/O
        "TcpStream::", "TcpListener::", "UdpSocket::",
        ".connect(", ".bind(", ".listen(",
        // Environment I/O
        "env::var", "env::args",
        // Process I/O
        "Command::", ".spawn(", ".output(",
    ];
    io_patterns.iter().any(|p| line.contains(p))
}

fn detect_alloc_effect(line: &str) -> bool {
    // IMPORTANT: .clone() and .collect() are NOT included!
    // - .clone() on Copy types doesn't allocate
    // - .collect() output varies
    // For strict tracking, declare effects(alloc) explicitly
    let alloc_patterns = [
        "Vec::new", "Vec::with_capacity",
        "String::new", "String::from", "String::with_capacity",
        "Box::new", "Rc::new", "Arc::new",
        "HashMap::new", "HashSet::new", "BTreeMap::new",
        "vec!", "format!",
        ".to_string()", ".to_owned()", ".to_vec()",
    ];
    alloc_patterns.iter().any(|p| line.contains(p))
}

fn detect_panic_effect(line: &str) -> bool {
    let panic_patterns = [
        "panic!", ".unwrap()", ".expect(",
        "assert!", "assert_eq!", "assert_ne!",
        "unreachable!", "unimplemented!", "todo!",
    ];
    panic_patterns.iter().any(|p| line.contains(p))
}

fn detect_write_effect(line: &str, param: &str) -> bool {
    // Detects: param.field = value
    // Does NOT detect: field = param.value (struct field init)
    let pattern = format!("{}.", param);
    if line.contains(&pattern) {
        // Check for assignment after field access
        // ... field mutation detection logic
    }
    false
}
```

#### What's NOT Detected (By Design)

| Pattern | Why Not Detected |
|---------|------------------|
| `.clone()` | Copy types don't allocate; user declares if needed |
| `.collect()` | Output type varies; may not allocate |
| `Point { x: 0 }` | Stack-allocated struct literal |
| `(1, 2, 3)` | Stack-allocated tuple |
| `[1, 2, 3]` | Stack-allocated fixed array |

This conservative approach **eliminates false positives** while maintaining strict effect tracking for definite effects.

### 1.1 Type-Driven Effect Inference (Roadmap)

**Current State:** Pattern-based detection (regex/string matching)  
**Target:** Type-driven structural inference

```rust
// CURRENT: Heuristic detection
fn detect_alloc_effect(line: &str) -> bool {
    line.contains("Vec::new") || line.contains("Box::new")  // Fragile!
}

// TARGET: Type-based inference from HIR
fn infer_effects(expr: &HirExpr, type_env: &TypeEnv) -> EffectSet {
    match expr {
        HirExpr::Call { func, args } => {
            let func_type = type_env.lookup(func);
            func_type.effect_signature()  // From type, not string!
        }
        HirExpr::FieldMut { base, .. } => {
            if type_env.is_param(base) {
                EffectSet::write(base.binding_id())
            } else {
                EffectSet::empty()
            }
        }
        // ...
    }
}
```

**Deliverables:**
- [ ] Type environment untuk semua expressions
- [ ] Effect signature di function types
- [ ] Inference algorithm yang structural
- [ ] Unit tests untuk setiap inference rule

### Scope Analysis Algorithm

```rust
// Simplified scope analysis
fn analyze_assignment(&mut self, var: &str, line: usize) {
    // 1. Check if var exists in current scope
    if self.in_current_scope(var) {
        // Same-scope reassignment → Error RSPL071 if not mut
        if !self.is_mutable(var) {
            emit_error(RSPL071, var, line);
        }
        return;
    }
    
    // 2. Check if var exists in outer scope
    if self.in_outer_scope(var) {
        // Ambiguous shadowing → Error RSPL081
        emit_error(RSPL081, var, line);
        return;
    }
    
    // 3. New declaration
    self.declare(var, line);
}
```

### Rust Sanity Gate

```rust
pub fn check_rust_output(code: &str) -> SanityCheckResult {
    let mut errors = Vec::new();
    
    // Check 1: Balanced delimiters
    errors.extend(check_balanced_delimiters(code));
    
    // Check 2: Illegal tokens (bare mut without let)
    errors.extend(check_illegal_tokens(code));
    
    // Check 3: Unclosed strings
    errors.extend(check_unclosed_strings(code));
    
    // Check 4: Effect annotation leakage
    errors.extend(check_effect_leakage(code));
    
    SanityCheckResult { is_valid: errors.is_empty(), errors }
}
```

---

## Contributing

### Development Setup

```bash
# Clone
git clone https://github.com/rustsp/rustsp
cd rustsp

# Build
cargo build

# Test
cargo test

# Run specific test
cargo test test_logic06
```

### Code Style

- Use `rustfmt` for formatting
- Add doc comments for public functions
- Include tests for new features
- Follow existing naming conventions

### Adding New Logic Rules

1. Add new variant to `LogicViolation` enum
2. Implement detection logic in `anti_fail_logic.rs`
3. Add error code to `error_msg.rs`
4. Add tests
5. Update documentation

### Adding New Transformations

1. Add transformation function to appropriate module
2. Integrate into `lib.rs` lowering pipeline
3. Add sanity check if needed
4. Add tests
5. Update documentation

---

## 📄 License

MIT License - see [LICENSE](LICENSE) for details.

---

## 🙏 Acknowledgments

- **Rust Team** - For creating Rust and inspiring memory safety
- **Effect Systems Research** - Academic foundations for effect tracking
- **DSDN Project** - Real-world use case driving development

---

<div align="center">

**RustS+** - *Where Logic Safety Meets Memory Safety*

*"If Rust prevents segmentation faults, RustS+ prevents logical faults."*

</div>