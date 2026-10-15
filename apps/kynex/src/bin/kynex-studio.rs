//! `kynex-studio`: KYNEX as a program of its own, in a window of its own
//! (see `kynex::studio`). No console: it is what the Start menu starts.

#![windows_subsystem = "windows"]

fn main() {
    kynex::studio::run();
}
