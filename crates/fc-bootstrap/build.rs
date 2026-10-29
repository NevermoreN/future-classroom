fn main() {
    // Cargo does not otherwise track edits to a custom linker wrapper. Rebuild
    // the final binary when it changes instead of reusing a stale linked EXE.
    println!("cargo:rerun-if-changed=../../scripts/link-win7-gnu.sh");
}
