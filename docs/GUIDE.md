# RustS+ Language Specification v0.9

> **RustS+** adalah superset Rust yang dirancang untuk mencegah **bug logika** dengan tingkat keseriusan yang sama seperti Rust mencegah **bug memori**. Dokumen ini adalah **spesifikasi normatif** — setiap aturan di sini di-enforce oleh compiler.

---

## Daftar Isi

1. [Filosofi Inti](#1-filosofi-inti)
2. [Pipeline Compiler](#2-pipeline-compiler)
3. [Sistem Variabel](#3-sistem-variabel)
4. [Scope dan Block Semantics](#4-scope-dan-block-semantics)
5. [Effect Ownership Model](#5-effect-ownership-model)
6. [Function Semantics](#6-function-semantics)
7. [Struct dan Enum](#7-struct-dan-enum)
8. [Control Flow sebagai Ekspresi](#8-control-flow-sebagai-ekspresi)
9. [Error Codes Reference](#9-error-codes-reference)
10. [Cargo Integration](#10-cargo-integration)
11. [Lowering ke Rust](#11-lowering-ke-rust)

---

## 1. Filosofi Inti

### 1.1 Tujuan RustS+

RustS+ adalah **lapisan bahasa (superset)** di atas Rust dengan tujuan:

| Layer | Penjaga | Dicegah |
|-------|---------|---------|
| Rust | Memory Safety | Use-after-free, double-free, data races |
| RustS+ | Logic Safety | Hidden effects, ambiguous intent, dishonest code |

**Prinsip Fundamental:**

1. **Tidak ada perubahan state tanpa niat eksplisit**
2. **Tidak ada efek samping tersembunyi**  
3. **Tidak ada shadowing ambigu**
4. **Tidak ada logika "terasa benar tapi salah"**

### 1.2 Kode Tidak Jujur Tidak Pernah Dikompilasi

RustS+ menerapkan filosofi **"Honest Code Only"**:

- Jika fungsi melakukan efek → **WAJIB** mendeklarasikannya
- Jika variabel di-reassign → **WAJIB** menggunakan `mut`
- Jika modifikasi variabel outer scope → **WAJIB** menggunakan `outer`

Kode yang melanggar aturan ini **TIDAK AKAN** diteruskan ke Rust compiler.

### 1.3 Semantic Compiler, Bukan Text Transformer

RustS+ **bukan** sekadar "bahasa dengan sintaks baru" yang ditransform via regex. RustS+ adalah **sistem formal untuk menjamin kebenaran makna program**.

```
┌───────────────────────────────────────────────────────────────────┐
│  MISCONCEPTION: RustS+ adalah regex/text transformer               │
│  ─────────────────────────────────────────────────────────────    │
│  ❌ Source → Regex Replace → Rust                                  │
│                                                                    │
│  REALITY: RustS+ adalah semantic compiler                          │
│  ─────────────────────────────────────────────────────────────    │
│  ✅ Source → AST → HIR → EIR → Validated Rust                      │
│              ↑      ↑     ↑                                        │
│           struktur makna effect                                    │
└───────────────────────────────────────────────────────────────────┘
```

**Mengapa ini penting?**

| Approach | Problem |
|----------|---------|
| Regex-based | Tidak memahami context, mudah salah parse |
| AST-only | Tidak memahami scope dan binding resolution |
| **HIR + EIR** | Memahami **makna** dan **effect** secara formal |

RustS+ membangun **tiga layer IR**:

1. **AST (Abstract Syntax Tree)** - Struktur sintaks
2. **HIR (High-level IR)** - Resolved bindings, scope, mutability  
3. **EIR (Effect IR)** - Effect inference, propagation, ownership

Dengan arsitektur ini, RustS+ dapat mendeteksi kesalahan **semantik** (bukan hanya sintaks) sebelum satu baris Rust pun dihasilkan.

---

## 2. Pipeline Compiler

### 2.1 Diagram Pipeline

```
┌─────────────────────────────────────────────────────────────────────┐
│  STAGE 0: EFFECT & FUNCTION ANALYSIS                                │
│    → Parse semua function signatures dengan effect declarations     │
│    → Build function table dengan effect contracts                   │
│    → Build effect dependency graph untuk cross-function checking    │
├─────────────────────────────────────────────────────────────────────┤
│  STAGE 1: ANTI-FAIL LOGIC CHECK                                     │
│    → Logic-01: Expression completeness (if/match branches)          │
│    → Logic-02: Ambiguous shadowing detection                        │
│    → Logic-03: Illegal statements in expression context             │
│    → Logic-04: Implicit mutation detection                          │
│    → Logic-05: Unclear intent patterns                              │
│    → Logic-06: Same-scope reassignment without mut                  │
│    → Effect-01: Undeclared effect validation                        │
│    → Effect-02: Effect leak detection                               │
│    → Effect-03: Pure calling effectful detection                    │
│    → Effect-04: Cross-function effect propagation                   │
│    → Effect-05: Effect scope validation                             │
│    → Effect-06: Effect ownership validation                         │
│                                                                     │
│    ⚠️  JIKA ADA PELANGGARAN → KOMPILASI BERHENTI DI SINI            │
│    ⚠️  KODE RUST TIDAK AKAN DIHASILKAN                              │
├─────────────────────────────────────────────────────────────────────┤
│  STAGE 2: LOWERING (RustS+ → Rust)                                  │
│    → Transform sintaks RustS+ ke Rust valid                         │
│    → Strip effects clause dari signatures                           │
│    → Transform parameter types ([T] → &[T])                         │
│    → Add #[derive(Clone)] untuk value semantics                     │
│    → RUST SANITY GATE: Validasi output Rust                         │
├─────────────────────────────────────────────────────────────────────┤
│  STAGE 3: RUST COMPILATION (rustc)                                  │
│    → Compile generated Rust ke binary                               │
│    → Map rustc errors kembali ke RustS+ source                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 2.2 Stage 1: Anti-Fail Logic (CRITICAL)

Stage 1 adalah **gerbang utama** RustS+. Semua pengecekan logika terjadi di sini:

| Check | Rule | Dicegah |
|-------|------|---------|
| Logic-01 | Expression completeness | `if` tanpa `else` saat digunakan sebagai value |
| Logic-02 | Ambiguous shadowing | Assignment ke outer variable tanpa `outer` keyword |
| Logic-03 | Statement in expression | `let` statement di dalam expression context |
| Logic-04 | Implicit mutation | Field mutation tanpa tracking |
| Logic-05 | Unclear intent | Empty blocks, ambiguous patterns |
| Logic-06 | Same-scope reassignment | Reassignment tanpa `mut` declaration |
| Effect-01 | Undeclared effect | Fungsi melakukan efek yang tidak dideklarasikan |
| Effect-02 | Effect leak | Efek bocor ke closure tanpa propagation |
| Effect-03 | Pure calling effectful | Fungsi pure memanggil fungsi effectful |
| Effect-04 | Missing propagation | Efek dari callee tidak dipropagasi |
| Effect-05 | Effect scope violation | Efek dilakukan di luar scope yang valid |
| Effect-06 | Effect ownership conflict | Dua sumber efek menulis state yang sama |

### 2.3 Rust Sanity Gate

Sebelum kode dikirim ke rustc, RustS+ menjalankan **Sanity Gate**:

- Validasi balanced delimiters: `()`, `[]`, `{}`
- Validasi tidak ada `mut x = ...` tanpa `let`
- Validasi tidak ada effect annotations yang bocor (`effects(...)`)
- Validasi tidak ada unclosed strings

Jika Sanity Gate gagal → **INTERNAL COMPILER ERROR** (bukan error Rust).

---

## 3. Sistem Variabel

### 3.1 Deklarasi Variabel

Dalam RustS+, `let` **tidak wajib**. Assignment adalah deklarasi:

```rust
// RustS+
a = 10

// Diturunkan ke Rust:
let a = 10;
```

### 3.2 Same-Scope Reassignment (Logic-06)

**ATURAN:** Reassignment ke variabel di scope yang sama **WAJIB** menggunakan `mut`.

❌ **INVALID:**
```rust
fn main() {
    x = 10
    x = x + 1    // ERROR! Reassignment tanpa mut
}
```

Compiler error:
```
error[RSPL071][scope]: reassignment to `x` without `mut` declaration
  --> main.rss:3:5
    |
3   |     x = x + 1
    |     ^^^^^^^^^

note:
  Logic-06 VIOLATION: Same-Scope Reassignment

  variable `x` was first assigned on line 2.
  reassigning without `mut` is not allowed in RustS+.

help:
  change original declaration to:

    mut x = ...
```

✔ **VALID:**
```rust
fn main() {
    mut x = 10      // Declare sebagai mutable
    x = x + 1       // OK - sudah mut
}
```

### 3.3 Shadowing vs Reassignment

| Konsep | Definisi | Contoh |
|--------|----------|--------|
| **Assignment** | Deklarasi binding baru | `x = 10` |
| **Reassignment** | Mengubah binding yang sudah ada | `mut x = 10; x = 20` |
| **Shadowing** | Membuat binding baru dengan nama sama | `x = 10; { x = "hello" }` |

**ATURAN:** RustS+ **TIDAK** mengizinkan reassignment tanpa `mut`.

**ATURAN:** Shadowing di inner scope akan membuat variabel **BARU**. Outer variable **TIDAK** berubah.

```rust
a = 10
{
    a = "inner"    // Ini adalah SHADOWING, bukan reassignment
    // Inner `a` adalah String, outer `a` tetap i32
}
println(a)         // Output: 10 (outer tidak berubah)
```

### 3.4 Type Inference

RustS+ melakukan type inference dari nilai:

| Value | Inferred Type |
|-------|---------------|
| `"hello"` | `String` |
| `42` | `i32` |
| `3.14` | `f64` |
| `true`/`false` | `bool` |
| `'c'` | `char` |

---

## 4. Scope dan Block Semantics

### 4.1 Block Scope Rules

Setiap `{}` membuat scope baru. Assignment di inner scope **default-nya** membuat variabel baru (shadowing).

```rust
fn main() {
    x = 10
    {
        x = 20     // SHADOWING - outer x tidak berubah
    }
    // x masih 10
}
```

### 4.2 Ambiguous Shadowing (Logic-02)

**ATURAN:** Assignment ke nama yang sudah ada di outer scope **AKAN ERROR** karena ambigu.

❌ **INVALID:**
```rust
fn main() {
    counter = 0
    {
        counter = counter + 1    // ERROR! Ambiguous shadowing
    }
}
```

Compiler error:
```
error[RSPL081][scope]: ambiguous shadowing of outer variable `counter`
  --> main.rss:4:9
    |
4   |         counter = counter + 1
    |         ^^^^^^^^^^^^^^^^^^^^^

note:
  Logic-02 VIOLATION: Ambiguous Shadowing

  in RustS+, assignment in inner block creates NEW variable by default.
  outer `counter` will NOT change after this block.
  use `outer counter` to modify the outer variable.

help:
  use `outer counter = ...` to modify outer variable
```

### 4.3 Outer Mutation Keyword

Untuk memodifikasi variabel dari scope luar, gunakan keyword `outer`:

✔ **VALID:**
```rust
fn main() {
    mut counter = 0
    {
        outer counter = counter + 1    // Eksplisit modifikasi outer
    }
    // counter sekarang 1
}
```

**Syntax:**
```rust
outer <var_name> = <expression>
```

**ATURAN:** `outer` **WAJIB** digunakan saat ingin memodifikasi variabel dari scope luar.

---

## 5. Effect Ownership Model

### 5.1 Konsep Dasar

RustS+ mengimplementasi **borrow checker untuk makna program** melalui Effect System. Sama seperti Rust melarang dua mutable owner atas memori yang sama, RustS+ melarang dua sumber efek yang tidak terkoordinasi atas state yang sama.

### 5.2 Effect Types

| Effect | Syntax | Deskripsi |
|--------|--------|-----------|
| `read(param)` | `effects(read x)` | Fungsi membaca dari parameter |
| `write(param)` | `effects(write x)` | Fungsi memutasi parameter |
| `io` | `effects(io)` | Fungsi melakukan I/O (println!, read, write) |
| `alloc` | `effects(alloc)` | Fungsi mengalokasi memori (Vec::new, Box::new) |
| `panic` | `effects(panic)` | Fungsi mungkin panic (unwrap, expect, panic!) |

### 5.3 Effect Declaration Syntax

```rust
fn function_name(params) effects(effect1, effect2, ...) ReturnType {
    body
}
```

**Contoh:**
```rust
fn transfer(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance - amount
    acc
}

fn log(msg String) effects(io) {
    println("{}", msg)
}
```

### 5.4 Function Classification

| Classification | Definisi |
|----------------|----------|
| **PURE** | Tidak ada efek. Referentially transparent. |
| **EFFECTFUL** | Memiliki satu atau lebih efek yang dideklarasikan. |

```rust
// PURE - tidak ada efek
fn add(a i32, b i32) i32 {
    a + b
}

// EFFECTFUL - memiliki efek io
fn greet(name String) effects(io) {
    println("Hello, {}", name)
}
```

### 5.5 Effect Rules (WAJIB)

#### Rule 1: Effect Honesty (Effect-01)

**ATURAN:** Jika fungsi melakukan efek, fungsi **WAJIB** mendeklarasikannya.

❌ **INVALID:**
```rust
fn save(data String) {     // Tidak ada deklarasi efek
    println("Saving...")   // ERROR! I/O effect tidak dideklarasi
}
```

Compiler error:
```
error[RSPL300][effect]: function `save` performs effect `io` but does not declare it
  --> main.rss:1:4
    |
1   | fn save(data String) {
    |    ^^^^

note:
  Effect-01 VIOLATION: Undeclared Effect

  in RustS+, functions must HONESTLY declare their effects.
  the function `save` performs `io` but this is not in its signature.

help:
  add effect declaration to function signature:

  fn save(...) effects(io) { ... }
```

✔ **VALID:**
```rust
fn save(data String) effects(io) {
    println("Saving...")
}
```

#### Rule 2: Effect Propagation (Effect-04)

**ATURAN:** Jika fungsi A memanggil fungsi B yang memiliki efek propagatable, A **WAJIB** mendeklarasikan efek tersebut.

**Propagatable effects:** `io`, `alloc`, `panic`

❌ **INVALID:**
```rust
fn inner() effects(io) {
    println("inner")
}

fn outer() {           // ERROR! Tidak mendeklarasi io
    inner()            // inner() memiliki efek io
}
```

Compiler error:
```
error[RSPL301][effect]: function `outer` calls `inner` which has effect `io` but does not propagate it
  --> main.rss:5:4
    |
5   | fn outer() {
    |    ^^^^^

note:
  Effect-04 VIOLATION: Missing Effect Propagation

  `outer` calls `inner` which declares effects: io
  these effects must be propagated to the caller.

help:
  add effect declaration:

  fn outer(...) effects(io) { ... }
```

✔ **VALID:**
```rust
fn inner() effects(io) {
    println("inner")
}

fn outer() effects(io) {   // Propagate efek dari inner
    inner()
}
```

#### Rule 3: Pure Calling Effectful (Effect-03)

**ATURAN:** Fungsi pure **TIDAK BOLEH** memanggil fungsi effectful secara langsung.

❌ **INVALID:**
```rust
fn logger() effects(io) {
    println("log")
}

fn compute(x i32) i32 {    // Pure function
    logger()               // ERROR! Pure calling effectful
    x * 2
}
```

#### Rule 4: Effect Ownership (Effect-06)

**ATURAN:** Dua fungsi berbeda **TIDAK BOLEH** menulis ke parameter yang sama tanpa koordinasi.

### 5.6 Effect vs Rust Output

**CRITICAL:** Effect annotations adalah **compile-time contracts**. Mereka **TIDAK PERNAH** muncul di output Rust.

```rust
// RustS+ Source:
fn apply_tx(w Wallet, tx Tx) effects(write w) Wallet {
    // ...
}

// Rust Output (effect stripped):
fn apply_tx(w: Wallet, tx: Tx) -> Wallet {
    // ...
}
```

### 5.7 Special Case: main() Function

Fungsi `main()` **diizinkan** memiliki implicit `io` effect untuk kenyamanan.

```rust
fn main() {
    println("Hello")    // OK - main() memiliki implicit io
}
```

### 5.8 Effect Inference: Bagaimana Compiler Mendeteksi Effect

RustS+ menggunakan **Effect Inference Algorithm** yang berjalan di atas HIR (High-level IR). Ini **bukan regex/text matching** — compiler memahami struktur program secara formal.

#### Aturan Inferensi Effect

| Ekspresi | Effect yang Diinfer | Penjelasan |
|----------|--------------------|-----------| 
| `42`, `"hello"`, `true` | ∅ (kosong) | Literal tidak punya efek |
| `x` (baca variabel) | `read(x)` | Membaca binding |
| `param.field` | `read(param)` | Akses field = baca owner |
| `param.field = value` | `write(param)` | **Mutasi field = mutasi owner** |
| `param = new_value` | ∅ (kosong) | Rebinding ≠ mutasi isi |
| `println!(...)` | `io` | I/O operation |
| `Vec::new()`, `Box::new()` | `alloc` | Memory allocation |
| `.unwrap()`, `panic!()` | `panic` | May panic |
| `f(args)` | `effects(f) ∪ effects(args)` | Union caller + callee |
| `if c { a } else { b }` | `effects(c) ∪ effects(a) ∪ effects(b)` | Union semua branch |

#### Key Insight: Field Mutation = Owner Mutation

**PENTING:** Mutasi terhadap **field** dianggap sebagai mutasi terhadap **owner object**.

```rust
struct Account {
    id u64
    balance i64
}

fn deposit(acc Account, amount i64) effects(write acc) Account {
    // acc.balance = ... ← compiler infer sebagai write(acc)
    // karena mengubah field = mengubah state keseluruhan object
    acc.balance = acc.balance + amount
    acc
}
```

#### Rebinding vs Mutation

```rust
fn rebind_example(w Wallet) Wallet {
    // Ini adalah REBINDING, bukan mutation
    // w = new_wallet TIDAK menghasilkan write(w)
    // karena kita mengganti binding, bukan mengubah isi
    w = Wallet { id = 1, balance = 0 }
    w
}

fn mutation_example(w Wallet) effects(write w) Wallet {
    // Ini adalah MUTATION
    // w.balance = ... MENGHASILKAN write(w)
    // karena kita mengubah isi object yang existing
    w.balance = w.balance + 100
    w
}
```

### 5.9 Best Practices: Menulis Kode dengan Effect System

#### ✅ DO: Deklarasikan Semua Effect Secara Eksplisit

```rust
// BAIK: Effect dideklarasi dengan jelas
fn save_to_disk(data String) effects(io) {
    write_file("data.txt", data)
}

fn process_and_log(item Item) effects(io, alloc) {
    results = Vec::new()        // alloc
    results.push(transform(item))
    println("Processed: {}", item.id)  // io
}
```

#### ✅ DO: Pisahkan Pure dan Effectful Functions

```rust
// BAIK: Pure function terpisah
fn calculate_total(items [Item]) i64 {
    items.iter().map(|i| i.price).sum()
}

// BAIK: Effectful function terpisah
fn display_total(items [Item]) effects(io) {
    total = calculate_total(items)  // Call pure function
    println("Total: {}", total)      // Effect hanya di sini
}
```

#### ✅ DO: Propagasi Effect dari Callee

```rust
fn helper() effects(io) {
    println("helper called")
}

// BAIK: main() mempropagasi io dari helper()
fn process() effects(io) {
    helper()  // Caller harus declare effect dari callee
}
```

#### ❌ DON'T: Menyembunyikan Effect

```rust
// BURUK: Effect tersembunyi
fn sneaky_function(x i32) i32 {
    println("called with {}", x)  // ERROR! io tidak dideklarasi
    x * 2
}
```

#### ❌ DON'T: Pure Function Memanggil Effectful Function

```rust
fn effectful() effects(io) {
    println("effect!")
}

// BURUK: Pure function memanggil effectful
fn supposedly_pure() i32 {
    effectful()  // ERROR! RSPL302
    42
}
```

#### ✅ DO: Gunakan write(param) untuk Mutasi Parameter

```rust
struct State {
    counter i32
    data String
}

// BAIK: write(state) menunjukkan state akan dimutasi
fn increment(state State) effects(write state) State {
    state.counter = state.counter + 1
    state
}

// BAIK: Multiple writes jelas
fn transfer(from Account, to Account, amount i64) 
    effects(write from, write to) 
    (Account, Account) 
{
    from.balance = from.balance - amount
    to.balance = to.balance + amount
    (from, to)
}
```

#### Pattern: Functional Core, Effectful Shell

```rust
// PURE CORE - semua logic di sini
fn apply_transaction(wallet Wallet, tx Transaction) Wallet {
    match tx {
        Transaction::Deposit { amount } {
            Wallet { id = wallet.id, balance = wallet.balance + amount }
        }
        Transaction::Withdraw { amount } {
            Wallet { id = wallet.id, balance = wallet.balance - amount }
        }
    }
}

// EFFECTFUL SHELL - I/O dan interaksi dunia luar
fn main() effects(io) {
    wallet = Wallet { id = 1, balance = 100 }
    tx = read_transaction_from_user()  // io
    
    new_wallet = apply_transaction(wallet, tx)  // pure!
    
    println("New balance: {}", new_wallet.balance)  // io
}
```

---

## 6. Function Semantics

### 6.1 Function Syntax

RustS+ menggunakan syntax yang lebih bersih dari Rust:

```rust
// RustS+
fn add(a i32, b i32) i32 {
    a + b
}

// Diturunkan ke Rust:
fn add(a: i32, b: i32) -> i32 {
    a + b
}
```

**Perbedaan utama:**
- Parameter: `name Type` (bukan `name: Type`)
- Return type langsung setelah `)` (bukan `-> Type`)

### 6.2 Single-Line Functions

Fungsi pendek bisa ditulis dalam satu baris:

```rust
fn double(x i32) i32 = x * 2
```

### 6.3 Generic Functions

Generics menggunakan `[]` bukan `<>`:

```rust
// RustS+
fn identity[T](x T) T {
    x
}

// Diturunkan ke Rust:
fn identity<T>(x: T) -> T {
    x
}
```

### 6.4 Parameter Ownership

| Syntax | Ownership |
|--------|-----------|
| `x T` | Move (transfer ownership) |
| `x &T` | Immutable borrow |
| `x &mut T` | Mutable borrow |

### 6.5 Slice Parameters

**ATURAN:** Bare slice type `[T]` sebagai parameter **otomatis** ditransform ke `&[T]`.

```rust
// RustS+ Source:
fn process(items [Item]) {
    // ...
}

// Rust Output:
fn process(items: &[Item]) {
    // ...
}
```

### 6.6 Return Value

- Ekspresi terakhir = return value (tanpa `;`)
- Fungsi void tidak memiliki return type
- `()` sebagai return type akan di-strip

```rust
fn compute(x i32) i32 {
    x * 2       // Return value (tanpa ;)
}

fn log(msg String) effects(io) () {
    println("{}", msg)
}
// Diturunkan ke: fn log(msg: String) { ... }
```

---

## 7. Struct dan Enum

### 7.1 Struct Definition

```rust
struct Node {
    id u32
    balance i64
    name String
}

// Diturunkan ke Rust:
#[derive(Clone)]
struct Node {
    id: u32,
    balance: i64,
    name: String,
}
```

**ATURAN:** Semua struct mendapat `#[derive(Clone)]` otomatis untuk value semantics.

### 7.2 Struct Instantiation

```rust
node = Node {
    id = 1
    balance = 100
    name = "Alice"
}

// Diturunkan ke Rust:
let node = Node {
    id: 1,
    balance: 100,
    name: String::from("Alice"),
};
```

**Transformasi:**
- `field = value` → `field: value`
- String literal → `String::from("...")`

### 7.3 Enum Definition

```rust
enum Event {
    Init(Node)
    Credit { id u32, amount i64 }
    Debit { id u32, amount i64 }
    Query(u32)
}

// Diturunkan ke Rust:
#[derive(Clone)]
enum Event {
    Init(Node),
    Credit { id: u32, amount: i64 },
    Debit { id: u32, amount: i64 },
    Query(u32),
}
```

### 7.4 Enum Instantiation

```rust
ev = Event::Credit { id = 1, amount = 500 }

// Diturunkan ke Rust:
let ev = Event::Credit { id: 1, amount: 500 };
```

---

## 8. Control Flow sebagai Ekspresi

### 8.1 if/else sebagai Ekspresi

Semua `if` adalah **ekspresi** yang menghasilkan nilai:

```rust
result = if x > 0 {
    "positive"
} else {
    "negative"
}
```

### 8.2 Expression Completeness (Logic-01)

**ATURAN:** Jika `if` digunakan sebagai value, **WAJIB** memiliki `else` branch.

❌ **INVALID:**
```rust
result = if x > 0 {
    "positive"
}
// ERROR! Missing else branch
```

Compiler error:
```
error[RSPL060][control-flow]: `if` expression used as value must have `else` branch
```

✔ **VALID:**
```rust
result = if x > 0 {
    "positive"
} else {
    "negative"
}
```

### 8.3 Match Expression

RustS+ menggunakan syntax match yang lebih bersih (tanpa `=>`):

```rust
// RustS+
match status {
    "ok" {
        1
    }
    "error" {
        -1
    }
    _ {
        0
    }
}

// Diturunkan ke Rust:
match status.as_str() {
    "ok" => {
        1
    },
    "error" => {
        -1
    },
    _ => {
        0
    },
}
```

### 8.4 Match dengan Enum Destructuring

```rust
match ev {
    Event::Credit { id, amount } {
        if id == target_id {
            process_credit(amount)
        } else {
            skip()
        }
    }
    Event::Debit { id, amount } {
        process_debit(id, amount)
    }
    _ {
        ignore()
    }
}
```

---

## 9. Error Codes Reference

### 9.1 Logic Errors (RSPL001-019)

| Code | Deskripsi |
|------|-----------|
| RSPL001 | Generic logic error |
| RSPL002 | Unreachable code detected |
| RSPL003 | Infinite loop detected |

### 9.2 Structure Errors (RSPL020-039)

| Code | Deskripsi |
|------|-----------|
| RSPL020 | Invalid function signature |
| RSPL021 | Invalid struct definition |
| RSPL022 | Invalid enum definition |
| RSPL023 | Missing function body |
| RSPL024 | Duplicate definition |
| RSPL025 | Invalid field syntax |
| RSPL026 | Missing type annotation |

### 9.3 Expression Errors (RSPL040-059)

| Code | Deskripsi |
|------|-----------|
| RSPL040 | Expression used as statement |
| RSPL041 | Statement used as expression |
| RSPL042 | Invalid assignment target |
| RSPL043 | Missing value in expression context |
| RSPL044 | Type mismatch in expression |
| RSPL045 | Invalid operator usage |
| RSPL046 | String literal where String expected |

### 9.4 Control Flow Errors (RSPL060-079)

| Code | Deskripsi |
|------|-----------|
| RSPL060 | If expression missing else branch |
| RSPL061 | Match expression missing arms |
| RSPL062 | Match arm type mismatch |
| RSPL063 | Unreachable match arm |
| RSPL064 | Non-exhaustive match |
| RSPL065 | Invalid guard expression |
| RSPL066 | Break outside loop |
| RSPL067 | Continue outside loop |
| RSPL068 | Return outside function |
| **RSPL071** | **Same-scope reassignment without mut** |

### 9.5 Scope Errors (RSPL080-099)

| Code | Deskripsi |
|------|-----------|
| RSPL080 | Variable not found in scope |
| **RSPL081** | **Ambiguous shadowing (outer variable)** |
| RSPL082 | Outer keyword on non-existent variable |
| RSPL083 | Variable used before initialization |
| RSPL084 | Scope leak attempt |
| RSPL085 | Invalid outer mutation target |

### 9.6 Effect System Errors (RSPL300-349)

| Code | Deskripsi |
|------|-----------|
| **RSPL300** | **Undeclared effect performed** |
| **RSPL301** | **Missing effect propagation** |
| **RSPL302** | **Pure function calling effectful** |
| RSPL303 | Effect leak to closure |
| RSPL304 | Conflicting effect declarations |
| RSPL305 | Invalid effect syntax |
| RSPL306 | Effect on non-parameter |
| RSPL307 | Write effect without read |
| RSPL308 | Effect scope violation |
| RSPL309 | Concurrent effect conflict |
| RSPL310 | Effect not allowed in context |
| RSPL311 | Missing panic effect |
| RSPL312 | Missing io effect |
| RSPL313 | Missing alloc effect |
| RSPL314 | Effect contract violation |
| RSPL315 | Effect ownership violation |
| RSPL316 | Effect borrow violation |

---

## 10. Cargo Integration

### 10.1 Instalasi cargo-rustsp

`cargo-rustsp` adalah tool untuk mengintegrasikan RustS+ dengan Cargo workflow.

```bash
# Build cargo-rustsp
rustc cargo-rustsp.rs -o cargo-rustsp

# Install ke PATH
cp cargo-rustsp ~/.cargo/bin/
```

### 10.2 Project Structure

```
my_project/
├── Cargo.toml
└── src/
    ├── main.rss      # RustS+ source (bukan .rs)
    ├── lib.rss       # Library module
    └── utils.rss     # Other modules
```

### 10.3 Commands

| Command | Deskripsi |
|---------|-----------|
| `cargo rustsp build` | Compile project |
| `cargo rustsp run` | Build dan run |
| `cargo rustsp test` | Run tests |
| `cargo rustsp check` | Check tanpa compile |
| `cargo rustsp clean` | Clean artifacts |

### 10.4 Options

```bash
cargo rustsp build --release     # Release build
cargo rustsp run -- arg1 arg2    # Pass arguments
```

### 10.5 Workflow

```
cargo rustsp build
    │
    ├─→ Scan src/ untuk .rss files
    │
    ├─→ Compile setiap .rss ke .rs (via rustsp)
    │   └─→ Stage 0: Effect Analysis
    │   └─→ Stage 1: Anti-Fail Logic ← ERROR STOPS HERE
    │   └─→ Stage 2: Lowering
    │
    ├─→ Copy .rs files ke shadow directory
    │
    ├─→ Generate Cargo.toml
    │
    └─→ Run cargo build di shadow directory
```

### 10.6 Shadow Directory

cargo-rustsp menggunakan **shadow directory** di TEMP untuk mengisolasi build:

```
/tmp/rustsp_shadow_<project_name>/
├── Cargo.toml           # Generated
└── src/
    ├── main.rs          # Compiled dari main.rss
    └── lib.rs           # Compiled dari lib.rss
```

Target artifacts tetap di `target/rustsp_build/` dalam project directory.

---

## 11. Lowering ke Rust

### 11.1 Syntax Transformations

| RustS+ | Rust |
|--------|------|
| `fn foo(x i32) i32 {` | `fn foo(x: i32) -> i32 {` |
| `fn foo[T](x T) T {` | `fn foo<T>(x: T) -> T {` |
| `effects(io) ()` | *(stripped)* |
| `x = 10` | `let x = 10;` |
| `mut x = 10` | `let mut x = 10;` |
| `struct S { x i32 }` | `#[derive(Clone)] struct S { x: i32, }` |

### 11.2 Effect Stripping

Effect annotations di-strip saat lowering:

```rust
// RustS+
fn apply(w Wallet) effects(write w) Wallet { w }

// Rust Output
fn apply(w: Wallet) -> Wallet { w }
```

### 11.3 Automatic Transformations

| Transformation | Contoh |
|----------------|--------|
| Slice to ref | `[T]` → `&[T]` |
| String literal coercion | `"hello"` → `String::from("hello")` |
| Slice index clone | `arr[i]` → `arr[i].clone()` |
| Call-site borrow | `f(arr)` → `f(&arr)` (jika param `&[T]`) |
| Derive Clone | `struct S {}` → `#[derive(Clone)] struct S {}` |
| Macro bang | `println(x)` → `println!(x)` |

### 11.4 Statement Transformations

| RustS+ Statement | Rust Output |
|------------------|-------------|
| `effect write(x)` | *(skipped entirely)* |
| `outer x = y` | `x = y;` |
| Match arm `Pattern { body }` | `Pattern => { body },` |

---

## Appendix A: Quick Reference Card

### Variables
```rust
x = 10              // Deklarasi
mut x = 10          // Mutable declaration
x = x + 1           // ERROR tanpa mut
outer x = y         // Modify outer scope
```

### Functions
```rust
fn pure(a i32) i32 { a }                    // Pure function
fn effectful() effects(io) { println("!") } // Effectful
fn generic[T](x T) T { x }                  // Generic
```

### Effects
```rust
effects(io)              // I/O operations
effects(write x)         // Mutate parameter x
effects(read x)          // Read parameter x
effects(alloc)           // Memory allocation
effects(panic)           // May panic
effects(io, write x)     // Multiple effects
```

### Control Flow
```rust
if cond { a } else { b }    // If expression
match x { P { body } }      // Match (no =>)
while cond { body }         // While loop
```

---

## Appendix B: Differences from Rust

| Aspect | Rust | RustS+ |
|--------|------|--------|
| Variable declaration | `let x = 10;` | `x = 10` |
| Mutable variable | `let mut x = 10;` | `mut x = 10` |
| Function param | `x: i32` | `x i32` |
| Return type | `-> i32` | `i32` (langsung setelah `)`) |
| Generics | `<T>` | `[T]` |
| Match arm | `=> { }` | `{ }` |
| Effect declaration | N/A | `effects(...)` |
| Outer mutation | N/A | `outer x = ...` |

---

## Appendix C: Effect System Checklist

Sebelum compile, pastikan:

- [ ] Setiap fungsi yang melakukan I/O memiliki `effects(io)`
- [ ] Setiap fungsi yang memutasi parameter memiliki `effects(write param)`
- [ ] Setiap fungsi yang memanggil effectful function mempropagasi efeknya
- [ ] Tidak ada fungsi pure yang memanggil effectful function
- [ ] Effect annotations menggunakan syntax yang benar

---

## Appendix D: Formal Effect Type Signatures

### Function Type dalam RustS+

Setiap fungsi di RustS+ secara formal memiliki tipe:

```
(parameter types) → return type + capability set
```

### Contoh Type Signatures

```rust
// Type: (i32, i32) → i32 + ∅
fn add(a i32, b i32) i32 { a + b }

// Type: (String) → () + {io}
fn log(msg String) effects(io) { println("{}", msg) }

// Type: (Account, i64) → Account + {write(acc)}
fn deposit(acc Account, amount i64) effects(write acc) Account {
    acc.balance = acc.balance + amount
    acc
}

// Type: (Account, Account, i64) → (Account, Account) + {write(from), write(to)}
fn transfer(from Account, to Account, amount i64) 
    effects(write from, write to) 
    (Account, Account) 
{
    from.balance = from.balance - amount
    to.balance = to.balance + amount
    (from, to)
}
```

### write(x) sebagai Linear Resource

Capability `write(x)` diperlakukan seperti `&mut T` di Rust:

| Property | &mut T (Rust) | write(x) (RustS+) |
|----------|---------------|-------------------|
| Exclusive | ✅ Satu &mut per waktu | ✅ Satu write per waktu |
| Must transfer | ✅ Harus dipinjam/dikembalikan | ✅ Harus dipropagasi |
| Compile-time | ✅ Checked saat compile | ✅ Checked saat compile |

### Effect Propagation Rules

```
┌────────────────────────────────────────────────────────────────────┐
│  RULE: Jika A memanggil B, maka:                                   │
│        effects(A) ⊇ propagatable_effects(B)                        │
│                                                                    │
│  propagatable_effects = {io, alloc, panic}                         │
│  (read/write adalah parameter-bound, tidak dipropagasi)            │
└────────────────────────────────────────────────────────────────────┘
```

```rust
fn inner() effects(io, alloc) {
    println("hello")
    data = Vec::new()
}

// WAJIB: outer harus declare io DAN alloc
fn outer() effects(io, alloc) {
    inner()  // propagates io, alloc
}

// ERROR: missing alloc propagation
fn wrong() effects(io) {
    inner()  // RSPL301: missing alloc propagation
}
```

---

**RustS+ Language Specification v0.9**

*"Jika Rust mencegah segmentation fault, RustS+ mencegah logical fault."*
