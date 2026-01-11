# RustS+ (rustsp-Rlang)

**RustS+** adalah bahasa sistem generasi baru yang dibangun di atas Rust, dengan tujuan utama:

> *Mencegah bug logika dengan tingkat keseriusan yang sama seperti Rust mencegah bug memori.*

RustS+ bukan bahasa yang menggantikan Rust.  
RustS+ adalah **cara baru menulis Rust** — lebih sederhana, lebih jujur, dan lebih aman secara semantik.

---

## 🧠 Filosofi

**Rust** mencegah:
- segmentation fault  
- use-after-free  
- data race  

**RustS+** mencegah:
- logic race  
- ambiguous mutation  
- shadowing yang tidak disadari  
- perubahan state tanpa niat eksplisit  

Jika Rust melindungi **memori**,  
RustS+ melindungi **makna program**.

---

## 🧩 Arsitektur

RustS+ adalah **superset dari Rust**.

Pipeline kompilasi:

.rss (RustS+ source)
│
▼
RustS+ compiler (logic + intent checker)
│
▼
Rust (.rs)
│
▼
rustc → LLVM → machine code

yaml
Salin kode

Jika analisis logika gagal, kode **tidak akan pernah** diteruskan ke Rust.

Ini menciptakan dua lapisan keamanan:
- **Logic safety (RustS+)**
- **Memory safety (Rust)**

---

## ✨ Contoh

### Rust

```rust
let mut a = 10;
a = a + 1;
RustS+
rusts
Salin kode
a = 10
a = a + 1
RustS+ akan:

menentukan mut secara otomatis

mencegah shadowing ambigu

memastikan niat programmer eksplisit

📦 Struct & Enum
rusts
Salin kode
struct Node {
    id u32
    balance i64
}

enum Event {
    Init(Node)
    Credit { id u32, amount i64 }
    Debit { id u32, amount i64 }
}
rusts
Salin kode
node = Node {
    id = 1
    balance = 100
}
🔀 Control Flow as Expression
rusts
Salin kode
status = if balance > 1000 {
    "rich"
} else if balance >= 0 {
    "normal"
} else {
    "debt"
}
rusts
Salin kode
match status {
    "rich" {
        println("rich")
    }
    _ {
        println("other")
    }
}
Semua if dan match adalah ekspresi.

🧠 Anti-Fail Logic
RustS+ memperkenalkan Effect Ownership:

Fungsi default-nya pure

Mutasi dan efek harus eksplisit

Tidak ada perubahan state tersembunyi

Seperti Rust melarang dua mutable reference,
RustS+ melarang dua sumber efek tanpa kontrak eksplisit.

🚀 Cargo Integration (Planned)
RustS+ dirancang untuk hidup di dalam ekosistem Cargo:

bash
Salin kode
cargo rustsp build
cargo rustsp run
cargo build --rustsp
Satu project bisa mencampur:

.rs (Rust)

.rss (RustS+)

dalam satu crate.