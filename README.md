# RustS+ (RustSPlus)

**The Programming Language with Effect Honesty**

*Rust mencegah memory bugs. RustS+ mencegah logic bugs.*


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

## 🎯 What is RustS+?

**RustS+** adalah **superset** dari Rust yang menambahkan lapisan **Logic Safety** di atas **Memory Safety** Rust. RustS+ memperkenalkan konsep **Effect Ownership** — sebuah sistem yang memaksa programmer untuk jujur tentang apa yang dilakukan kode mereka.

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

## 💭 Philosophy

### The Problem RustS+ Solves

Rust mencegah **memory bugs** — use-after-free, double-free, data races. Tapi Rust tidak mencegah **logic bugs**:

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

RustS+ memaksa kejujuran:

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

1. **Effect Honesty**: Jika fungsi melakukan efek → WAJIB deklarasi
2. **Intent Clarity**: Tidak ada ambiguitas tentang apa yang kode lakukan
3. **Explicit State**: Semua perubahan state harus eksplisit
4. **No Hidden Mutations**: Assignment = deklarasi baru, bukan mutasi diam-diam
5. **Compile-Time Enforcement**: Semua aturan di-enforce sebelum runtime

---

## 🚀 Quick Start

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

Buat file `hello.rss`:

```rust
fn main() effects(io) {
    println("Hello, RustS+!")
}
```

Compile dan run:

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

## 🧠 Formal IR Pipeline

RustS+ bukan sekadar "bahasa dengan sintaks baru" — ini adalah **sistem formal untuk menjamin kebenaran makna program**. Arsitekturnya dibangun di atas rangkaian **Intermediate Representation (IR)** formal:

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

### Mengapa IR Formal?

Dengan arsitektur ini, RustS+ menjadi **semantic compiler** yang memahami apa yang dilakukan program secara formal, bukan sekadar **text transformer**:

| Approach | Problem |
|----------|---------|
| Regex/Text-based | Tidak memahami context, mudah salah |
| AST-only | Tidak memahami scope dan binding |
| **HIR + EIR** | Memahami makna dan effect secara formal |

---

## 🎭 Two-Layer Type System

RustS+ memiliki **Type System dua-lapis**:

```
┌───────────────────────────────────────────────────────────────┐
│  LAYER 2: EFFECT CAPABILITY SYSTEM                            │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  read(x)  │  write(x)  │  io  │  alloc  │  panic        │  │
│  │                                                         │  │
│  │  "Setiap nilai tidak hanya memiliki tipe data,          │  │
│  │   tetapi juga HAK atas realitas"                        │  │
│  └─────────────────────────────────────────────────────────┘  │
├───────────────────────────────────────────────────────────────┤
│  LAYER 1: RUST TYPE SYSTEM                                    │
│  ┌─────────────────────────────────────────────────────────┐  │
│  │  i32, String, struct, enum, borrow, generics, lifetimes │  │
│  └─────────────────────────────────────────────────────────┘  │
└───────────────────────────────────────────────────────────────┘
```

### Effect as Linear Resource

**Capability `write(x)` diperlakukan sebagai linear resource** — sama seperti `&mut T` di Rust:

- **Tidak boleh diduplikasi** — hanya satu pihak yang boleh memiliki `write(x)` pada satu waktu
- **Harus dipropagasi** — jika fungsi memiliki write capability, caller harus declare atau propagate
- **Exclusive ownership** — dua fungsi tidak boleh sama-sama menulis state yang sama tanpa koordinasi

```rust
// write(acc) adalah "exclusive write token" untuk acc
fn deposit(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance + amount  // OK - memiliki write token
    acc
}

fn withdraw(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance - amount  // OK - memiliki write token
    acc
}

// ERROR: Dua write token untuk acc di jalur eksekusi yang sama
// akan terdeteksi sebagai RSPL315: Effect ownership violation
```

### Function Type Signature

Setiap fungsi di RustS+ secara formal bertipe:

```
(parameter types) → return type + capability set
```

Contoh:
```rust
fn transfer(from Account, to Account, amount i64) 
    effects(write from, write to) 
    (Account, Account)
    
// Type signature formal:
// (Account, Account, i64) → (Account, Account) + {write(from), write(to)}
```

---

## ⚙️ Compilation Pipeline

RustS+ menggunakan **4-stage compilation pipeline**:

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

## 🛡️ The Anti-Fail Logic System

Anti-Fail Logic adalah jantung dari RustS+. Sistem ini terdiri dari **6 Logic Rules** dan **6 Effect Rules**.

### Logic Rules

#### Logic-01: Expression Completeness

`if`/`match` yang digunakan sebagai value WAJIB memiliki semua branch.

```rust
// ❌ INVALID - missing else
result = if x > 0 { "positive" }

// ✅ VALID
result = if x > 0 { "positive" } else { "negative" }
```

**Error Code:** `RSPL060`

#### Logic-02: Ambiguous Shadowing

Assignment ke variabel outer scope tanpa `outer` keyword akan ERROR.

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

`let` statement tidak boleh muncul di expression context.

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

Mutasi field struct harus bisa di-track.

#### Logic-05: Unclear Intent

Pattern yang membingungkan seperti empty blocks `{}` akan di-flag.

**Error Code:** `RSPL001`

#### Logic-06: Same-Scope Reassignment

Reassignment di scope yang sama WAJIB menggunakan `mut`.

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

Jika fungsi melakukan efek, WAJIB deklarasi.

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

Effect tidak boleh bocor ke closure tanpa propagation.

#### Effect-03: Pure Calling Effectful

Fungsi pure TIDAK BOLEH memanggil fungsi effectful.

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

Effect dari callee WAJIB dipropagasi ke caller.

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

Effect harus dilakukan dalam scope yang valid.

#### Effect-06: Concurrent Effect Conflict

Dua sumber effect tidak boleh menulis state yang sama.

---

## 🎭 Effect Ownership Model

### Concept: Borrow Checker for Program Meaning

Sama seperti Rust memiliki borrow checker untuk memory, RustS+ memiliki **effect checker** untuk program meaning.

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

| Effect | Syntax | Description | Propagatable |
|--------|--------|-------------|--------------|
| `io` | `effects(io)` | I/O operations (println, read, write) | ✅ Yes |
| `alloc` | `effects(alloc)` | Memory allocation (Vec::new, Box::new) | ✅ Yes |
| `panic` | `effects(panic)` | May panic (unwrap, expect, panic!) | ✅ Yes |
| `read(x)` | `effects(read x)` | Read from parameter x | ❌ No |
| `write(x)` | `effects(write x)` | Write/mutate parameter x | ❌ No |

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

RustS+ menggunakan **Effect Inference Algorithm** yang berjalan di atas HIR. Setiap ekspresi dan statement menghasilkan **jejak efek** yang dihitung secara **struktural**, bukan berbasis teks/regex:

| Expression/Statement | Inferred Effect | Reasoning |
|---------------------|-----------------|-----------|
| `42`, `"hello"`, `true` | ∅ (none) | Literal tidak menghasilkan efek |
| `x` (read variable) | `read(x)` | Membaca binding menghasilkan read |
| `w.field` | `read(w)` | Akses field = read owner object |
| `w.field = 3` | `write(w)` | Mutasi field = mutasi owner |
| `w = new_w` | ∅ (none) | Rebinding ≠ mutasi isi |
| `println!(...)` | `io` | I/O operation (AST-level pattern) |
| `Vec::new()` | `alloc` | Memory allocation |
| `.unwrap()` | `panic` | May panic |
| `f(args...)` | `effects(f) ∪ effects(args)` | Gabungan caller + callee |
| `if c { a } else { b }` | `effects(c) ∪ effects(a) ∪ effects(b)` | Union semua branch |

**Key Insight:** Mutasi terhadap **field** (`w.x = 3`) dianggap sebagai mutasi terhadap **owner object** (`write(w)`). Ini karena perubahan field mengubah *state* keseluruhan object.

```rust
fn update_balance(acc Account, delta i64) effects(write acc) Account {
    // acc.balance = ... menghasilkan write(acc)
    // karena field mutation = owner mutation
    acc.balance = acc.balance + delta
    acc
}
```

### Effect Dependency Graph

RustS+ builds a **dependency graph** untuk cross-function effect analysis:

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

---

## 📖 Syntax Reference

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

## 🛠️ Cargo Integration

### Apa itu cargo-rustsp?

`cargo-rustsp` adalah **build tool** yang mengintegrasikan RustS+ compiler dengan ekosistem Cargo. Dengan cargo-rustsp, kamu bisa menggunakan workflow Cargo yang familiar (`cargo build`, `cargo run`, dll) untuk project RustS+.

```
┌─────────────────────────────────────────────────────────────────────┐
│                        cargo-rustsp v0.9.0                          │
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
│  │  • Incremental compilation (hash-based caching)              │   │
│  │  • Mixed .rs/.rss projects                                   │   │
│  │  • Feature flags support                                     │   │
│  │  • Source-mapped error reporting                             │   │
│  └──────────────────────────────────────────────────────────────┘   │
│                                                                     │
└─────────────────────────────────────────────────────────────────────┘
```

### Fitur Utama

| Fitur | Deskripsi |
|-------|-----------|
| **Multi-Module** | Full support untuk nested modules (`mod foo;` resolves ke `foo.rss` atau `foo/mod.rss`) |
| **Workspace** | Build multiple crates dalam satu workspace |
| **Incremental** | Hash-based caching - hanya recompile file yang berubah |
| **Mixed Projects** | Gabungkan `.rs` (pure Rust) dan `.rss` (RustS+) dalam satu project |
| **Features** | Full `--features` support seperti cargo biasa |
| **Error Mapping** | Error messages menunjuk ke lokasi di file `.rss` asli |

### Installation

```bash
# Clone repository
git clone https://github.com/novenrizkia856-ui/rustsp-Rlang
cd rustsp-Rlang-main

# Build compiler dan cargo-rustsp
cargo build --release

# Install ke PATH
cp target/release/rustsp ~/.cargo/bin/
cp target/release/cargo-rustsp ~/.cargo/bin/

# Verifikasi instalasi
cargo rustsp --version
# Output: cargo-rustsp 0.9.0
```

### Commands

| Command | Description |
|---------|-------------|
| `cargo rustsp build` | Compile RustS+ project |
| `cargo rustsp run` | Build and run |
| `cargo rustsp test` | Run tests |
| `cargo rustsp check` | Check tanpa compile binary |
| `cargo rustsp clean` | Clean build artifacts |
| `cargo rustsp bench` | Run benchmarks |
| `cargo rustsp doc` | Generate documentation |

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

cargo-rustsp mengikuti aturan module resolution Rust:

```
mod foo;  →  Mencari dalam urutan:
             1. foo.rss      (RustS+ file)
             2. foo/mod.rss  (RustS+ directory module)
             3. foo.rs       (Rust file)
             4. foo/mod.rs   (Rust directory module)
```

Custom path dengan attribute:
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
│ 1. Analyze module graph               │
│    - Parse mod declarations           │
│    - Resolve all dependencies         │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 2. Check cache                        │
│    - Hash-based change detection      │
│    - Skip unchanged files             │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 3. Compile .rss files                 │
│    rustsp file.rss --emit-rs          │
│    (Stage 0 → Stage 1 → Stage 2)      │
│                                       │
│    ⚠️ ERROR? STOPS HERE               │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 4. Copy to shadow directory           │
│    /tmp/rustsp_shadow_<project>/      │
│    - .rs files (compiled dari .rss)   │
│    - .rs files (copy dari .rs asli)   │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 5. Generate Cargo.toml                │
└───────────────┬───────────────────────┘
                │
                ▼
┌───────────────────────────────────────┐
│ 6. cargo build                        │
│    Output: target/rustsp_build/       │
└───────────────────────────────────────┘
```

### Incremental Compilation

cargo-rustsp menyimpan cache untuk mempercepat rebuild:

```
target/
└── rustsp_build/
    ├── .rustsp_cache      # Hash-based cache file
    ├── debug/             # Debug build artifacts
    └── release/           # Release build artifacts
```

Cara kerja:
- Setiap file `.rss` di-hash berdasarkan content
- Jika hash sama dengan cache → skip compilation
- Force rebuild dengan `--force`

### Shadow Directory Isolation

cargo-rustsp menggunakan TEMP directory untuk menghindari konflik dengan parent Cargo.toml:

```
Original Project              Shadow Project (TEMP)
────────────────              ─────────────────────
my_project/                   /tmp/rustsp_shadow_my_project/
├── Cargo.toml                ├── Cargo.toml (generated)
└── src/                      └── src/
    ├── main.rss                  ├── main.rs (compiled)
    ├── utils.rss                 ├── utils.rs (compiled)
    └── helper.rs                 └── helper.rs (copied)
```

### Workspace Build

Untuk workspace dengan multiple crates:

```bash
# Build semua members yang punya .rss files
cargo rustsp build

# Build specific package
cargo rustsp build -p core

# Build all packages (termasuk pure Rust)
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
Pastikan kamu di directory yang berisi `Cargo.toml` atau subdirectory-nya.

#### "No .rss files found"
cargo-rustsp akan fallback ke plain `cargo` jika tidak ada file `.rss`.

#### "rustsp: command not found"
Pastikan `rustsp` compiler ada di PATH atau di directory yang sama dengan `cargo-rustsp`.

#### Cache Issues
Jika build terasa stale:
```bash
cargo rustsp clean
cargo rustsp build --force
```

#### Module Not Found
Pastikan struktur folder mengikuti konvensi:
- `mod foo;` → butuh `foo.rss` ATAU `foo/mod.rss`


## 🔬 Technical Deep Dive

### Lowering Implementation

Lowering adalah proses transformasi RustS+ syntax ke valid Rust. File `lib.rs` berisi implementasi utama.

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
// Effect detection patterns
fn detect_io_effect(line: &str) -> bool {
    line.contains("println!") ||
    line.contains("print!") ||
    line.contains("eprintln!") ||
    // ... more patterns
}

fn detect_write_effect(line: &str, param: &str) -> bool {
    // param.field = value
    let pattern = format!("{}.* =", param);
    // ... regex matching
}

fn detect_alloc_effect(line: &str) -> bool {
    line.contains("Vec::new") ||
    line.contains("Box::new") ||
    line.contains("String::new") ||
    // ... more patterns
}
```

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

## 🤝 Contributing

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
