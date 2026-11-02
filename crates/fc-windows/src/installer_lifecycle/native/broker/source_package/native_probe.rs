//! Explicit ignored native read-only probes. Each runs alone in a fresh process
//! whose test PE and fixed sibling MSI were derived by the external fixture.
//! Constants below identify the already-frozen MSVC product's14 ledger only.
//! No own MSI/template whole hash, runtime path or role is compiled/accepted.
use super::*;
const PRODUCT: &str = "9E50831D-7811-5039-8BC3-4934647448D8";
const PACKAGE: &str = "C1223EE2-A1BD-4B75-BD1D-9B25B1A372B5";
const VERSION: &str = "0.1.0";
const LEDGER: [(&str, u32, &str, &str); 14] = [
    (
        "LICENSE",
        1088,
        "32b48e0a752c101a54d4ea1affc589dd99d8b553c72bedba17c6e2e30d319b30",
        "BF8ECC92-DF5E-5031-AF8F-B122A05E64E6",
    ),
    (
        "MSI-CANDIDATE.txt",
        1813,
        "057bb922663a6fcfee4affe324c97354c9866a0874b3cc90a02dbcf4ccb6ce1c",
        "00CBA46A-3182-5F3E-A3C2-3B4D08BF8FCC",
    ),
    (
        "PROBE-README.txt",
        21941,
        "e5ee6a5c2843c98d66e496cd59797ba1fd759a5df5aec35f77c104e200e880f9",
        "1F2DCDC9-9F80-51BE-A10D-F294D45FDB05",
    ),
    (
        "THIRD-PARTY-NOTICES.txt",
        2448718,
        "0bf19c345f74c394a7ba30f0059db8bd1943d30d564165eefe3eb3205c2c72c9",
        "44E7E505-D3CB-5DC8-9730-50964BA3CC8C",
    ),
    (
        "fc-bootstrap.exe",
        10873856,
        "6a8e0c40d1c83158ca01d1bc01c573613ed0e6a980ddf7c345c0f9e94929483f",
        "0CA01C62-B71F-5C57-905B-B6C979ACB487",
    ),
    (
        "manifest.json",
        32262,
        "a079b46fffbe438d268832f6eae099580d24e3de31d33670c3efdb187dfff02a",
        "8D7F75D1-8AD6-5097-AEE4-62389B788737",
    ),
    (
        "web/THIRD-PARTY-NOTICES.txt",
        26033,
        "7e717fcd0c465c72e70e46d223b3a819b4b03a12c13877c896e194626815f33b",
        "51201E26-0CF3-5876-A591-91F68B3A94DB",
    ),
    (
        "web/assets/favicon-CXuYoSfB.svg",
        282,
        "46593a2ec46a77d65026d609dcd5630c447360522518d745458091a9df2f7f8a",
        "D0B2E913-0457-5E64-A66B-EC1EE7DA7138",
    ),
    (
        "web/assets/index-BkiZ2CBl.js",
        415401,
        "cdf0d8f9c8cfa259031cb22e10aacb1ffea7b1f7affb3a4235478069b2138bf3",
        "18B1A37C-EE86-5233-ACC2-773B48D189BB",
    ),
    (
        "web/assets/index-DrrJeLm9.css",
        39554,
        "fd6d963cdb889b3bffdebf631c310d869730a56474a389351fd7bdf49bdf385f",
        "B9ADCF83-BCB1-5F42-97D8-967FF84A306F",
    ),
    (
        "web/assets/pcm-worklet-B7E4LRoP.js",
        1724,
        "9934c276d98e30992dc8148ffc3a0dfdc178fed9aacbb8227255bede45c1fcfa",
        "28DB1C02-71DB-5DAF-8D0C-9844E24D16DC",
    ),
    (
        "web/assets/wav-worker-LSywWbpJ.js",
        884,
        "a0e364aa7c0d888d0f7118b86d873c95df95e461303546a8ad1d56d5038e1886",
        "C0EC8190-410A-52A3-895E-AD6564B75C1E",
    ),
    (
        "web/fc-assets.json",
        1073,
        "2b4034c8a0f0bfa73812a7f2853ba83b766772ff55096fc1e33c9f2ddb9e4593",
        "0C6686BF-2327-51F5-89A6-4243C1FD11CF",
    ),
    (
        "web/index.html",
        1232,
        "8761e3d65d2bee4fb860e14b315eec9e634a019b1633aea627c335397f8299f2",
        "58B108D4-3264-56B1-9DE5-19636D8437E2",
    ),
];
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn pristine_owned_state() {
    assert_eq!(
        CLAIMS.load(Ordering::Acquire),
        0,
        "no prior/native owned claim"
    );
    assert!(!FAULTED.load(Ordering::Acquire), "no unknown native close");
    assert!(
        RETAINED
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .all(|v| v.count == 0),
        "no retained original capabilities"
    );
}
#[test]
#[ignore = "Requires its exact canonical ABI2 GNU-testPE/MSI sibling fixture; readonly, no Begin"]
fn actual_fixed_source_capture_closes_all_msi_before_begin() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    pristine_owned_state();
    let mut session = Session::new(Instant::now() + Duration::from_secs(120))
        .expect("actual fixed source readonly capture");
    let source = session
        .captured_source
        .as_ref()
        .expect("ABI2 typed source capture, not legacy");
    assert_eq!(source.abi, 2);
    assert_eq!(source.package, PACKAGE);
    assert_eq!(source.family, INSTALL_FAMILY);
    assert_eq!(source.manifest.product, PRODUCT);
    assert_eq!(source.manifest.version, VERSION);
    assert_eq!(source.manifest.entries.len(), 14);
    for (entry, expected) in source.manifest.entries.iter().zip(LEDGER) {
        assert_eq!(entry.path, expected.0);
        assert_eq!(entry.bytes, expected.1);
        assert_eq!(hex(&entry.sha256), expected.2);
        assert_eq!(entry.component, expected.3);
    }
    assert_eq!(session.sealed_package_sha, Some(source.source_sha256));
    // Re-read the actual current image's original pinned32-byte slot instead
    // of relying only on the Session field derived after capture succeeded.
    let image_handle = session
        .resources
        .handles
        .iter()
        .rev()
        .find(|v| v.role == Role::Parent)
        .expect("original current image pin")
        .handle;
    let digest_at = carrier_image::hole(source.template_bytes, |at, out| {
        session.resources.read_at(image_handle, at as i64, out)
    })
    .unwrap();
    let mut actual_seal = [0; 32];
    session
        .resources
        .read_at(image_handle, digest_at as i64, &mut actual_seal)
        .unwrap();
    assert_eq!(actual_seal, source.source_sha256);

    assert_ne!(source.source_sha256, [0; 32]);
    assert_ne!(source.template_sha256, [0; 32]);
    assert_eq!(session.image_sha, source.image_sha256);
    assert!(source.source_bytes > 0 && source.source_bytes <= MAX_SOURCE);
    assert_ne!(source.source_object, (0, 0));
    let image = super::super::image_path(unsafe { GetCurrentProcess() }).unwrap();
    let image_length = std::fs::metadata(image).unwrap().len();
    assert_eq!(source.template_bytes as u64, image_length);
    let decoded = payload_manifest::parse(&source.envelope).unwrap();
    assert_eq!(decoded.package(), PACKAGE);
    assert_eq!(decoded.payload().product, PRODUCT);
    assert_eq!(decoded.payload().version, VERSION);
    assert_eq!(
        source.close_receipts.len(),
        10,
        "db+summary+3views+5records actually closed"
    );
    assert!(source
        .close_receipts
        .iter()
        .all(|v| v.handle != 0 && v.close_return == 0 && v.view_return.is_none_or(|n| n == 0)));
    assert_eq!(
        source
            .close_receipts
            .iter()
            .filter(|v| v.view_return == Some(0))
            .count(),
        3
    );
    let source_sha = hex(&source.source_sha256);
    let template_sha = hex(&source.template_sha256);
    assert_eq!(
        CLAIMS.load(Ordering::Acquire).count_ones(),
        1,
        "same Session Resources claim only"
    );
    assert!(!session.resources.failed);
    // Resources contains no MSI namespace: these10 actual close receipts are
    // required BEFORE this kernel/CNG/pin release, not inferred from process exit.
    session.resources.release().unwrap();
    assert!(session.resources.released());
    pristine_owned_state();
    println!("SOURCE_PACKAGE_NATIVE_CAPTURE_OK abi=2 payloads=14 msiCloses=10 sourceSha={source_sha} templateSha={template_sha} claims=0");
}
fn rejects_known_invalid_source() {
    pristine_owned_state();
    match Session::new(Instant::now() + Duration::from_secs(120)) {
        Err(LifecycleError::Invalid) => {}
        Err(error) => panic!("expected known Invalid, got {error:?}"),
        Ok(mut session) => {
            session
                .resources
                .release()
                .expect("unexpected success resources still checkedclosed");
            panic!("wrong fixture accepted");
        }
    }
    // A synchronous format/hash rejection is known, not an unknown close. No
    // gate/Pending exists and the original Resources claim must return here.
    pristine_owned_state();
}
#[test]
#[ignore = "Requires fixed sibling MSI and deliberately wrong own32-byte seal fixture"]
fn sibling_msi_seal_mismatch_rejects_before_begin() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    rejects_known_invalid_source();
    println!("SOURCE_PACKAGE_NATIVE_SEAL_REJECT_OK claims=0");
}
#[test]
#[ignore = "Requires template outside-hole mutation and actual newly sealed MSI fixture"]
fn sibling_template_outside_digest_hole_rejects_before_begin() {
    let _serial = crate::NATIVE_TEST_SERIAL
        .lock()
        .unwrap_or_else(|p| p.into_inner());
    rejects_known_invalid_source();
    println!("SOURCE_PACKAGE_NATIVE_TEMPLATE_REJECT_OK claims=0");
}
