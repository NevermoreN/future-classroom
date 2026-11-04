fn main() {
    // Cargo does not otherwise track edits to a custom linker wrapper. Rebuild
    // the final binary when it changes instead of reusing a stale linked EXE.
    println!("cargo:rerun-if-changed=../../scripts/link-win7-gnu.sh");
    println!("cargo:rerun-if-changed=../../assets/icons/future-classroom.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    std::fs::copy(
        "../../assets/icons/future-classroom.ico",
        output.join("future-classroom.ico"),
    )
    .expect("copy generated application icon");
    std::fs::write(
        output.join("application.rc"),
        b"101 ICON \"future-classroom.ico\"\r\n",
    )
    .expect("write application icon resource");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    let (compiler, args, object) = if msvc {
        (
            "rc.exe".to_owned(),
            vec!["/nologo", "/foapplication.res", "application.rc"],
            "application.res",
        )
    } else {
        let target = std::env::var("TARGET").expect("Cargo TARGET");
        let compiler = if target.starts_with("i686-") {
            "i686-w64-mingw32-windres"
        } else if target.starts_with("x86_64-") {
            "x86_64-w64-mingw32-windres"
        } else {
            panic!("unsupported Windows icon resource target: {target}");
        };
        (
            compiler.to_owned(),
            vec!["-i", "application.rc", "-o", "application.o", "-O", "coff"],
            "application.o",
        )
    };
    let status = std::process::Command::new(&compiler)
        .args(args)
        .current_dir(&output)
        .status()
        .unwrap_or_else(|error| panic!("cannot run icon resource compiler {compiler}: {error}"));
    assert!(
        status.success(),
        "icon resource compilation failed: {status}"
    );
    // Both the resident application and the separately sealed Setup carrier use
    // the same generated resource. Library/test link targets remain unchanged.
    println!(
        "cargo:rustc-link-arg-bins={}",
        output.join(object).display()
    );
}
