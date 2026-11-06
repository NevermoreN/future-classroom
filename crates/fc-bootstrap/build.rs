fn main() {
    // Cargo does not otherwise track edits to a custom linker wrapper. Rebuild
    // the final binary when it changes instead of reusing a stale linked EXE.
    println!("cargo:rerun-if-changed=../../scripts/link-win7-gnu.sh");
    println!("cargo:rerun-if-changed=../../assets/icons/future-classroom.ico");
    println!("cargo:rerun-if-changed=../fc-domain/PRODUCT_VERSION");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo OUT_DIR"));
    std::fs::copy(
        "../../assets/icons/future-classroom.ico",
        output.join("future-classroom.ico"),
    )
    .expect("copy generated application icon");
    let version = std::fs::read_to_string("../fc-domain/PRODUCT_VERSION")
        .expect("read public product version");
    let fields: Vec<u16> = version
        .split('.')
        .map(|field| field.parse().expect("numeric file version"))
        .collect();
    assert!(fields.len() == 4, "four-field public file version");
    let numeric = fields
        .iter()
        .map(u16::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let resource = format!(
        r#"101 ICON "future-classroom.ico"
1 24 "application.manifest"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "FutureClassroom\0"
      VALUE "FileDescription", "FutureClassroom\0"
      VALUE "FileVersion", "{version}\0"
      VALUE "ProductName", "FutureClassroom\0"
      VALUE "ProductVersion", "{version}\0"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
    );
    std::fs::write(output.join("application.rc"), resource)
        .expect("write application icon resource");
    // Declare the current-user entry explicitly. Legacy installer detection
    // otherwise treats the sealed Setup.exe name as requiring elevation before
    // its native per-user installer can apply the actual Windows/MSI policy.
    std::fs::write(
        output.join("application.manifest"),
        br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <assemblyIdentity name="FutureClassroom.Application" version="1.0.0.0" type="win32" processorArchitecture="*" />
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false" />
      </requestedPrivileges>
    </security>
  </trustInfo>
</assembly>
"#,
    )
    .expect("write application execution-level manifest");
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
