================================================================================
                              RustS+ (RustSPlus)
            The Programming Language with Effect Honesty
================================================================================

  "Rust prevents memory bugs. RustS+ prevents logic bugs."

--------------------------------------------------------------------------------
  WHAT IS RUSTS+?
--------------------------------------------------------------------------------

RustS+ is a superset of Rust that adds a layer of logic safety on top of 
Rust's memory safety. RustS+ introduces the concept of effect ownership - 
a system that forces programmers to be honest about what their code does.

--------------------------------------------------------------------------------
  KEY FEATURES
--------------------------------------------------------------------------------

  * Effect Ownership   - Functions must declare side effects (io, write, alloc)
  * Anti-Fail Logic    - 6 logic rules + 6 effect rules enforced at compile time
  * Honest Code        - No hidden mutations, no surprise side effects
  * Clean Syntax       - Streamlined syntax without sacrificing safety
  * Rust Backend       - Compiles to native Rust, then to machine code

--------------------------------------------------------------------------------
  QUICK START
--------------------------------------------------------------------------------

After installation, create a file 'hello.rss':

    fn main() effects(io) {
        println("Hello, RustS+!")
    }

Compile and run:

    rustsp hello.rss -o hello
    hello.exe

--------------------------------------------------------------------------------
  REQUIREMENTS
--------------------------------------------------------------------------------

  * Windows 10/11 (64-bit)
  * Rust toolchain installed (rustc, cargo)
  * 50 MB free disk space

--------------------------------------------------------------------------------
  WHAT GETS INSTALLED
--------------------------------------------------------------------------------

  * rustsp.exe       - Main compiler executable
  * cargo-rustsp.exe - Cargo integration tool
  * Documentation    - README and examples (optional)

--------------------------------------------------------------------------------
  PROJECT INFO
--------------------------------------------------------------------------------

RustS+ is part of the DSDN (Data Semi-Decentral Network) project.

  Website:  https://github.com/novenrizkia856-ui/rustsp-Rlang
  License:  MIT License

================================================================================
                   Where Logic Safety Meets Memory Safety
================================================================================
