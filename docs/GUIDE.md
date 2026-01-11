# RustS+ Language Guide (Draft v0.1)

> **RustS+** adalah bahasa sistem berbasis Rust yang dirancang untuk mencegah **bug logika** dengan tingkat keseriusan yang sama seperti Rust mencegah **bug memori**. Dokumen ini menjelaskan *cara berpikir*, *cara menulis*, dan *aturan inti* RustS+, sehingga manusia maupun AI dapat mempelajarinya sebagai satu bahasa yang utuh.

---

## 1. Filosofi Inti RustS+

RustS+ bukan bahasa baru yang menggantikan Rust, melainkan **lapisan bahasa (superset)** di atas Rust.

* Rust = penjaga **keamanan memori**
* RustS+ = penjaga **kejujuran makna program (logic safety)**

Tujuan utama RustS+:

* Tidak ada perubahan state tanpa niat eksplisit
* Tidak ada efek samping tersembunyi
* Tidak ada shadowing ambigu
* Tidak ada logika "terasa benar tapi salah"

RustS+ selalu dikompilasi melalui dua tahap:

1. **Stage 1 – Logic & Intent Analysis (RustS+)**
2. **Stage 2 – Memory & Type Safety (Rust / rustc)**

Jika Stage 1 gagal, **kode tidak akan pernah diteruskan ke Rust**.

---

## 2. Anti-Fail Logic & Effect Ownership

Dalam RustS+, bug logika diperlakukan sebagai *pelanggaran kontrak niat*.

### 2.1 Effect Ownership

* Fungsi **default-nya murni (pure)**
* Perubahan state adalah **efek**
* Efek harus:

  * eksplisit
  * terlokalisasi
  * dapat diaudit

Seperti Rust melarang dua mutable owner atas memori yang sama,
RustS+ melarang dua sumber efek yang tidak terkoordinasi atas state yang sama.

### 2.2 Default Aman

```rusts
fn add(a i32, b i32) i32 {
    a + b
}
```

Fungsi di atas **tidak memiliki efek**.
RustS+ akan menolak efek tersembunyi di dalamnya.

---

## 3. Sistem Variabel RustS+

### 3.1 Deklarasi Variabel

* `let` **tidak wajib**
* Assignment = deklarasi

```rusts
a = 10
```

Diturunkan menjadi:

```rust
let a = 10;
```

### 3.2 Mutability Otomatis

Jika variabel diassign ulang:

```rusts
a = 10
a = 20
```

RustS+ otomatis menghasilkan:

```rust
let mut a = 10;
a = 20;
```

### 3.3 Shadowing

```rusts
a = 10
a = "hello"
```

Ini **bukan mutasi**, tapi **shadowing**:

```rust
let a = 10;
let a = String::from("hello");
```

Shadowing selalu eksplisit dan aman.

---

## 4. Scope & Block Semantics

* `{}` selalu membuat scope baru
* Assignment di block = **variable baru (default)**
* Tidak ada mutasi implisit ke scope luar

### 4.1 Shadowing Lokal

```rusts
a = 10
{
    a = "inner"
}
println(a)
```

Hasil:

```
10
```

### 4.2 Mutasi Variabel Luar

```rusts
outer a = a + 1
```

Tanpa `outer`, RustS+ **akan error**.

---

## 5. Control Flow sebagai Ekspresi

### 5.1 if / else

Semua `if` adalah **ekspresi**:

```rusts
result = if x > 0 {
    "positive"
} else {
    "negative"
}
```

Semua cabang **wajib menghasilkan nilai**.

### 5.2 else if

`else if` hanyalah chaining ekspresi:

```rusts
if x > 10 {
    "big"
} else if x > 0 {
    "small"
} else {
    "zero"
}
```

---

## 6. match Expression

`match` adalah ekspresi utama RustS+.

```rusts
match status {
    "ok" {
        1
    }
    _ {
        0
    }
}
```

* Tidak ada fallthrough
* Semua cabang wajib menghasilkan nilai
* Exhaustiveness tetap dijamin oleh Rust

---

## 7. Struct dalam RustS+

### 7.1 Definisi Struct

```rusts
struct Node {
    id u32
    balance i64
}
```

* Satu field per baris
* Urutan field = layout memori

### 7.2 Instansiasi

```rusts
node = Node {
    id = 1
    balance = 100
}
```

String literal otomatis diturunkan ke `String::from`.

### 7.3 Akses & Mutasi Field

```rusts
mut n = node
n.balance = n.balance + 10
```

Mutasi selalu eksplisit dan lokal.

---

## 8. Enum dalam RustS+

### 8.1 Definisi Enum

```rusts
enum Event {
    Init(Node)
    Credit { id u32, amount i64 }
    Debit { id u32, amount i64 }
}
```

### 8.2 Pattern Matching

```rusts
match ev {
    Event::Init(n) { n }
    Event::Credit { id, amount } { ... }
}
```

* Tidak ada simbol `=>`
* Tidak ada koma
* Semua cabang adalah ekspresi

---

## 9. Function & Parameter Semantics

### 9.1 Definisi Fungsi

```rusts
fn add(a i32, b i32) i32 {
    a + b
}
```

### 9.2 Return Value

* Ekspresi terakhir = return
* `;` di akhir fungsi non-void **dilarang**

### 9.3 Ownership Parameter

* `x T` → move
* `&T` → immutable borrow
* `&mut T` → mutable borrow

Semantik **identik dengan Rust**.

---

## 10. Error Message System

Error RustS+:

* manusiawi
* kontekstual
* menjelaskan *niat vs realita*

Contoh:

```
error[RSPL081][scope]: ambiguous shadowing of outer variable `x`

note:
  assignment di dalam block membuat variable BARU secara default

help:
  gunakan `outer x = ...` jika ingin memodifikasi scope luar
```

Rust error hanya muncul jika **benar-benar berasal dari Rust**, dan akan dipetakan ulang sejauh mungkin.

---

## 11. Hubungan dengan Rust

* Rust mentah **selalu boleh digunakan**
* RustS+ tidak membatasi fitur Rust
* RustS+ hanya menambah *kejujuran semantik*

RustS+ bukan bahasa alternatif terhadap Rust.
RustS+ adalah **cara manusia menulis Rust tanpa berbohong pada dirinya sendiri**.

Rust
let mut a = 10;
a = a + 1;
rusts
Salin kode
RustS+
a = 10
a = a + 1
---

## 12. Penutup

RustS+ bertujuan menjadi bahasa sistem yang:

* keras pada logika
* aman pada memori
* jujur pada niat
* ramah pada manusia

Jika Rust mencegah *segmentation fault*,
RustS+ mencegah *logical fault*.

Ini menjadikan RustS+ cocok untuk:

* infrastructure
* distributed systems
* consensus engine
* protocol & VM
* sistem kritikal seperti DSDN

**RustS+ Core v0.1 — Stable Foundation**
