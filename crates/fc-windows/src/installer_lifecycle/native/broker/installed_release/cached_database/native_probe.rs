//! Explicit installed-package native probe; compiled only in the test artifact.
//! Fixed assertions describe one test fixture, never PublishedPredecessor/permit.
use super::*;

#[test]
#[ignore = "Requires root-installed exact a470 MSVC package; run exact with --ignored"]
fn captures_real_installed_a470_cache_and_returns_every_owned_handle() {
    assert_eq!(
        CLAIMS.load(Ordering::Acquire),
        0,
        "isolated original process required"
    );
    assert!(!FAULTED.load(Ordering::Acquire));
    assert_eq!(retained_count(), 0);
    let original_thread = unsafe { GetCurrentThreadId() };
    let captured = InstalledReleaseReader::capture(Instant::now() + Duration::from_secs(120))
        .expect("real native cached database capture required, no substitution");
    let InstalledReleaseObservation::Single(captured) = captured else {
        panic!("fixed installed product is required; absence is not a passing probe");
    };
    assert_eq!(unsafe { GetCurrentThreadId() }, original_thread);
    assert_eq!(
        captured.product_code(),
        "9E50831D-7811-5039-8BC3-4934647448D8"
    );
    assert_eq!(captured.version(), [0, 1, 0]);
    assert_eq!(
        captured.sid(),
        "S-1-5-21-3896111207-1867804362-1971593122-1001"
    );
    assert!(captured.cache_bytes() > 0);
    assert_ne!(captured.cache_sha256(), [0; 32]);
    let db = captured.cached_database();
    assert_eq!(db.product, captured.product_code());
    assert_eq!(db.upgrade, INSTALL_FAMILY);
    assert_eq!(db.version, captured.version());
    assert_eq!(db.package, "C1223EE2-A1BD-4B75-BD1D-9B25B1A372B5");
    assert_eq!(db.template.bytes, 483768);
    assert_eq!(db.template.sha256, TEMPLATE_SHA);
    assert!(matches!(
        db.manifest_format,
        CapturedManifestFormat::LegacyWithoutPayloadManifest
    ));
    assert_eq!(
        db.binary_names,
        ["FcControlledGuardDll", "FcGuardClientTemplate"]
    );
    assert_eq!(db.components.len(), 14);
    assert_eq!(db.files.len(), 14);
    for expected in COMPONENTS_EXPECTED {
        let row = db
            .components
            .iter()
            .find(|r| r.component == expected.0)
            .expect("all14 component identities required");
        assert_eq!(row.component_id.as_deref(), Some(expected.1));
        assert_eq!(row.directory, expected.2);
        assert_eq!(row.attributes, 4);
        assert!(row.condition.is_none());
        assert_eq!(row.key_path.as_deref(), Some(expected.3));
    }
    for expected in FILES_EXPECTED {
        let row = db
            .files
            .iter()
            .find(|r| r.file == expected.0)
            .expect("all14 file identities required");
        assert_eq!(row.component, expected.1);
        assert_eq!(row.filename, expected.2);
        assert_eq!(row.size, expected.3);
        assert!(row.version.is_none());
        assert!(row.language.is_none());
        assert_eq!(row.attributes, 512);
        assert_eq!(row.sequence, expected.4);
    }
    assert_eq!(db.directories.len(), DIRECTORIES_EXPECTED.len());
    for (name, parent, default) in DIRECTORIES_EXPECTED {
        let row = db
            .directories
            .iter()
            .find(|r| r.directory == *name)
            .expect("complete raw Directory table");
        assert_eq!(row.parent.as_deref(), *parent);
        assert_eq!(row.default_dir, *default);
    }
    assert_eq!(db.custom_actions.len(), 1);
    let guard = &db.custom_actions[0];
    assert_eq!(guard.action, "FcControlledInnerGuard");
    assert_eq!(guard.kind, 1);
    assert_eq!(guard.source.as_deref(), Some("FcControlledGuardDll"));
    assert_eq!(guard.target.as_deref(), Some("FcControlledInnerGuard"));
    assert_eq!(db.execute_sequence.len(), EXECUTE_EXPECTED.len());
    for (action, condition, sequence) in EXECUTE_EXPECTED {
        let row = db
            .execute_sequence
            .iter()
            .find(|r| r.action == *action)
            .expect("complete raw InstallExecuteSequence table");
        assert_eq!(row.condition.as_deref(), *condition);
        assert_eq!(row.sequence, Some(*sequence));
    }
    let paths = database_rules::declared_directory_paths(&db.directories)
        .expect("closed per-user directory graph");
    assert_eq!(paths["FcInstallDir"], "");
    assert_eq!(paths["D4b5e57f6eb2f42b9039b3d1e"], "web");
    assert_eq!(paths["D7b144e7d5cf5146fdc8a96c4"], "web/assets");
    assert!(db
        .components
        .iter()
        .all(|r| paths.contains_key(&r.directory)));
    assert!(database_rules::fixed_guard_shape(
        &db.custom_actions,
        &db.execute_sequence,
        &db.binary_names
    ));
    // Original W5 asserted41. This revision adds three views and30 records;
    // every count here must be witnessed by this new actual native probe.
    assert_eq!(db.close_receipts.len(), 74);
    assert_eq!(
        db.close_receipts
            .iter()
            .filter(|r| matches!(r.kind, MsiKind::Database))
            .count(),
        1
    );
    assert_eq!(
        db.close_receipts
            .iter()
            .filter(|r| matches!(r.kind, MsiKind::Summary))
            .count(),
        1
    );
    assert_eq!(
        db.close_receipts
            .iter()
            .filter(|r| matches!(r.kind, MsiKind::View))
            .count(),
        8
    );
    assert_eq!(
        db.close_receipts
            .iter()
            .filter(|r| matches!(r.kind, MsiKind::Record))
            .count(),
        64
    );
    for receipt in &db.close_receipts {
        assert_ne!(receipt.handle, 0);
        assert_eq!(receipt.handle_close_return, 0);
        assert_eq!(
            receipt.view_close_return,
            if matches!(receipt.kind, MsiKind::View) {
                Some(0)
            } else {
                None
            }
        );
    }
    assert_eq!(
        CLAIMS.load(Ordering::Acquire),
        0,
        "original Resources owner must return its claim"
    );
    assert_eq!(retained_count(), 0);
    assert!(!FAULTED.load(Ordering::Acquire));
    // These are actual values from the original Rust API reader and its ledger.
    // Outer C# only captures this original process; it supplies no MSI facts.
    println!("FC_DB_PROBE_FACTS {:?}", captured);
    println!("FC_DB_PROBE_SUCCESS original_thread={} components={} files={} msi_closes={} claims={} retained={} faulted={}", original_thread, db.components.len(), db.files.len(), db.close_receipts.len(), CLAIMS.load(Ordering::Acquire), retained_count(), FAULTED.load(Ordering::Acquire));
}

const TEMPLATE_SHA: [u8; 32] = [
    161, 240, 183, 10, 119, 145, 38, 110, 227, 113, 224, 128, 33, 4, 93, 88, 184, 219, 5, 95, 146,
    180, 234, 146, 115, 95, 126, 255, 235, 108, 221, 147,
];
const COMPONENTS_EXPECTED: &[(&str, &str, &str, &str)] = &[
    (
        "Cc693279643b8cd5d248172d9",
        "{BF8ECC92-DF5E-5031-AF8F-B122A05E64E6}",
        "FcInstallDir",
        "Rc693279643b8cd5d248172d9",
    ),
    (
        "Ce61b854577338fd5a75cbdbf",
        "{00CBA46A-3182-5F3E-A3C2-3B4D08BF8FCC}",
        "FcInstallDir",
        "Re61b854577338fd5a75cbdbf",
    ),
    (
        "C74981d2baab2ca1978fa3139",
        "{1F2DCDC9-9F80-51BE-A10D-F294D45FDB05}",
        "FcInstallDir",
        "R74981d2baab2ca1978fa3139",
    ),
    (
        "Caf59ad43ec2920fd346dcd48",
        "{44E7E505-D3CB-5DC8-9730-50964BA3CC8C}",
        "FcInstallDir",
        "Raf59ad43ec2920fd346dcd48",
    ),
    (
        "Cb4591484ed1e8e854c88c63f",
        "{0CA01C62-B71F-5C57-905B-B6C979ACB487}",
        "FcInstallDir",
        "Rb4591484ed1e8e854c88c63f",
    ),
    (
        "Cffa5b716b5a57837f7929dfc",
        "{8D7F75D1-8AD6-5097-AEE4-62389B788737}",
        "FcInstallDir",
        "Rffa5b716b5a57837f7929dfc",
    ),
    (
        "C55abfed5453a84683c7f34ab",
        "{51201E26-0CF3-5876-A591-91F68B3A94DB}",
        "D4b5e57f6eb2f42b9039b3d1e",
        "R55abfed5453a84683c7f34ab",
    ),
    (
        "Cd04f9f0bbb15d02df958ca57",
        "{D0B2E913-0457-5E64-A66B-EC1EE7DA7138}",
        "D7b144e7d5cf5146fdc8a96c4",
        "Rd04f9f0bbb15d02df958ca57",
    ),
    (
        "Cbcf155810b3f3fc734c76f43",
        "{18B1A37C-EE86-5233-ACC2-773B48D189BB}",
        "D7b144e7d5cf5146fdc8a96c4",
        "Rbcf155810b3f3fc734c76f43",
    ),
    (
        "C15b357e3ccee0cd90f0305dd",
        "{B9ADCF83-BCB1-5F42-97D8-967FF84A306F}",
        "D7b144e7d5cf5146fdc8a96c4",
        "R15b357e3ccee0cd90f0305dd",
    ),
    (
        "Cf83899d1877e8d140ae1b17d",
        "{28DB1C02-71DB-5DAF-8D0C-9844E24D16DC}",
        "D7b144e7d5cf5146fdc8a96c4",
        "Rf83899d1877e8d140ae1b17d",
    ),
    (
        "C1afa55483054c67bd3d5b866",
        "{C0EC8190-410A-52A3-895E-AD6564B75C1E}",
        "D7b144e7d5cf5146fdc8a96c4",
        "R1afa55483054c67bd3d5b866",
    ),
    (
        "C42f1b2c2752f7b8628b04b1e",
        "{0C6686BF-2327-51F5-89A6-4243C1FD11CF}",
        "D4b5e57f6eb2f42b9039b3d1e",
        "R42f1b2c2752f7b8628b04b1e",
    ),
    (
        "C8f62b6ced28d3396b501d2e8",
        "{58B108D4-3264-56B1-9DE5-19636D8437E2}",
        "D4b5e57f6eb2f42b9039b3d1e",
        "R8f62b6ced28d3396b501d2e8",
    ),
];
const FILES_EXPECTED: &[(&str, &str, &str, i32, i32)] = &[
    (
        "Fc693279643b8cd5d248172d9",
        "Cc693279643b8cd5d248172d9",
        "LICENSE",
        1088,
        1,
    ),
    (
        "Fe61b854577338fd5a75cbdbf",
        "Ce61b854577338fd5a75cbdbf",
        "MSI-CANDIDATE.txt",
        1813,
        2,
    ),
    (
        "F74981d2baab2ca1978fa3139",
        "C74981d2baab2ca1978fa3139",
        "PROBE-README.txt",
        21941,
        3,
    ),
    (
        "Faf59ad43ec2920fd346dcd48",
        "Caf59ad43ec2920fd346dcd48",
        "THIRD-PARTY-NOTICES.txt",
        2448718,
        4,
    ),
    (
        "Fb4591484ed1e8e854c88c63f",
        "Cb4591484ed1e8e854c88c63f",
        "fc-bootstrap.exe",
        10873856,
        5,
    ),
    (
        "Fffa5b716b5a57837f7929dfc",
        "Cffa5b716b5a57837f7929dfc",
        "manifest.json",
        32262,
        6,
    ),
    (
        "F55abfed5453a84683c7f34ab",
        "C55abfed5453a84683c7f34ab",
        "THIRD-PARTY-NOTICES.txt",
        26033,
        7,
    ),
    (
        "Fd04f9f0bbb15d02df958ca57",
        "Cd04f9f0bbb15d02df958ca57",
        "favicon-CXuYoSfB.svg",
        282,
        8,
    ),
    (
        "Fbcf155810b3f3fc734c76f43",
        "Cbcf155810b3f3fc734c76f43",
        "index-BkiZ2CBl.js",
        415401,
        9,
    ),
    (
        "F15b357e3ccee0cd90f0305dd",
        "C15b357e3ccee0cd90f0305dd",
        "index-DrrJeLm9.css",
        39554,
        10,
    ),
    (
        "Ff83899d1877e8d140ae1b17d",
        "Cf83899d1877e8d140ae1b17d",
        "pcm-worklet-B7E4LRoP.js",
        1724,
        11,
    ),
    (
        "F1afa55483054c67bd3d5b866",
        "C1afa55483054c67bd3d5b866",
        "wav-worker-LSywWbpJ.js",
        884,
        12,
    ),
    (
        "F42f1b2c2752f7b8628b04b1e",
        "C42f1b2c2752f7b8628b04b1e",
        "fc-assets.json",
        1073,
        13,
    ),
    (
        "F8f62b6ced28d3396b501d2e8",
        "C8f62b6ced28d3396b501d2e8",
        "index.html",
        1232,
        14,
    ),
];

const DIRECTORIES_EXPECTED: &[(&str, Option<&str>, &str)] = &[
    (
        "D7b144e7d5cf5146fdc8a96c4",
        Some("D4b5e57f6eb2f42b9039b3d1e"),
        "assets",
    ),
    ("D4b5e57f6eb2f42b9039b3d1e", Some("FcInstallDir"), "web"),
    ("FcInstallDir", Some("UserPrograms"), "FutureClassroom"),
    ("UserPrograms", Some("LocalAppDataFolder"), "Programs"),
    ("LocalAppDataFolder", Some("TARGETDIR"), "."),
    ("TARGETDIR", None, "SourceDir"),
];
const EXECUTE_EXPECTED: &[(&str, Option<&str>, i32)] = &[
    ("CostInitialize", None, 800),
    ("FileCost", None, 900),
    ("CostFinalize", None, 1000),
    ("FindRelatedProducts", None, 25),
    ("LaunchConditions", None, 100),
    ("ValidateProductID", None, 700),
    ("MigrateFeatureStates", None, 1200),
    ("InstallValidate", None, 1400),
    ("InstallInitialize", None, 1500),
    ("ProcessComponents", None, 1600),
    ("UnpublishFeatures", None, 1800),
    ("RemoveRegistryValues", None, 2600),
    ("RemoveFiles", None, 3500),
    ("InstallFiles", None, 4000),
    ("WriteRegistryValues", None, 5000),
    ("RegisterUser", None, 6000),
    ("RegisterProduct", None, 6100),
    ("PublishFeatures", None, 6300),
    ("PublishProduct", None, 6400),
    ("InstallExecute", None, 6500),
    ("RemoveExistingProducts", None, 6501),
    ("InstallFinalize", None, 6600),
    ("FcControlledInnerGuard", Some("1"), 1499),
];
