use super::*;
use flate2::{write::DeflateEncoder, Compression};
use std::{
    io::{Cursor, Write},
    sync::Mutex,
};

static SERIAL: Mutex<()> = Mutex::new(());
struct Control;
impl ValidationControl for Control {
    fn canceled(&self) -> bool {
        false
    }
    fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
        Ok(Duration::ZERO)
    }
}
struct Part {
    name: String,
    body: Vec<u8>,
}
fn parts(kind: OfficeKind) -> Vec<Part> {
    let (local, ns, _) = xml::root_namespace(kind);
    vec![
        Part { name: "[Content_Types].xml".into(), body: format!("<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Override PartName=\"/{}\" ContentType=\"{}\"/></Types>", kind.main(), kind.content_type()).into_bytes() },
        Part { name: "_rels/.rels".into(), body: format!("<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"{}\"/></Relationships>",kind.main()).into_bytes() },
        Part { name: kind.main().into(), body: format!("<?xml version=\"1.0\"?><x:{local} xmlns:x=\"{ns}\"><x:body/></x:{local}>").into_bytes() },
    ]
}
fn push16(v: &mut Vec<u8>, n: u16) {
    v.extend(n.to_le_bytes());
}
fn push32(v: &mut Vec<u8>, n: u32) {
    v.extend(n.to_le_bytes());
}
fn archive(parts: &[Part], deflate: bool, descriptor: bool) -> Vec<u8> {
    let mut out = Vec::new();
    let mut central = Vec::new();
    for part in parts {
        let start = out.len() as u32;
        let crc = crc32fast::hash(&part.body);
        let data = if deflate {
            let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
            encoder.write_all(&part.body).unwrap();
            encoder.finish().unwrap()
        } else {
            part.body.clone()
        };
        let flags = 0x800 | if descriptor { 8 } else { 0 };
        let method = if deflate { 8 } else { 0 };
        push32(&mut out, 0x04034b50);
        push16(&mut out, 20);
        push16(&mut out, flags);
        push16(&mut out, method);
        push32(&mut out, 0);
        for n in [crc, data.len() as u32, part.body.len() as u32] {
            push32(&mut out, if descriptor { 0 } else { n });
        }
        push16(&mut out, part.name.len() as u16);
        push16(&mut out, 0);
        out.extend(part.name.bytes());
        out.extend(&data);
        if descriptor {
            push32(&mut out, 0x08074b50);
            for n in [crc, data.len() as u32, part.body.len() as u32] {
                push32(&mut out, n);
            }
        }
        push32(&mut central, 0x02014b50);
        push16(&mut central, 20);
        push16(&mut central, 20);
        push16(&mut central, flags);
        push16(&mut central, method);
        push32(&mut central, 0);
        for n in [crc, data.len() as u32, part.body.len() as u32] {
            push32(&mut central, n);
        }
        push16(&mut central, part.name.len() as u16);
        for _ in 0..4 {
            push16(&mut central, 0);
        }
        push32(&mut central, 0);
        push32(&mut central, start);
        central.extend(part.name.bytes());
    }
    let offset = out.len() as u32;
    let len = central.len() as u32;
    out.extend(central);
    push32(&mut out, 0x06054b50);
    push16(&mut out, 0);
    push16(&mut out, 0);
    push16(&mut out, parts.len() as u16);
    push16(&mut out, parts.len() as u16);
    push32(&mut out, len);
    push32(&mut out, offset);
    push16(&mut out, 0);
    out
}
fn check(data: Vec<u8>, kind: OfficeKind) -> Result<ContainerReport, ContainerError> {
    validate_office(&mut Cursor::new(data), kind, &Control)
}
fn central(data: &[u8]) -> usize {
    u32le(data, data.len() - 6) as usize
}

#[test]
fn real_stored_deflated_and_descriptor_packages_all_office_kinds() {
    let _guard = SERIAL.lock().unwrap();
    for kind in [OfficeKind::Docx, OfficeKind::Pptx, OfficeKind::Xlsx] {
        for deflate in [false, true] {
            for descriptor in [false, true] {
                let parts = parts(kind);
                let data = archive(&parts, deflate, descriptor);
                let expected: [u8; 32] = Sha256::digest(&data).into();
                let report = check(data.clone(), kind).unwrap();
                assert_eq!(report.sha256, expected);
                assert_eq!(report.entries, 3);
                assert_eq!(report.compressed_bytes, data.len() as u64);
                assert_eq!(
                    report.expanded_bytes,
                    parts.iter().map(|p| p.body.len() as u64).sum()
                );
                assert!(report.xml_events > 6);
            }
        }
    }
}

#[test]
fn utf16_main_and_namespace_aliases_are_decoded_before_limits() {
    let _guard = SERIAL.lock().unwrap();
    let mut p = parts(OfficeKind::Docx);
    let xml="<?xml version=\"1.0\" encoding=\"UTF-16\"?><w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\"><w:body><w:p>中文😀</w:p></w:body></w:document>";
    p[2].body = vec![0xff, 0xfe];
    p[2].body
        .extend(xml.encode_utf16().flat_map(u16::to_le_bytes));
    check(archive(&p, true, true), OfficeKind::Docx).unwrap();
    p[0].body = String::from_utf8(p[0].body.clone())
        .unwrap()
        .replace(
            "<Override ",
            "<c:Override xmlns:c=\"http://schemas.openxmlformats.org/package/2006/content-types\" ",
        )
        .into_bytes();
    check(archive(&p, false, false), OfficeKind::Docx).unwrap();
}

#[test]
fn traversal_case_aliases_windows_devices_and_executables_are_rejected() {
    let _guard = SERIAL.lock().unwrap();
    for (name, error) in [
        ("word/../evil.xml", ContainerError::UnsafePath),
        ("word/CON.xml", ContainerError::UnsafePath),
        ("word/COM¹.xml", ContainerError::UnsafePath),
        ("word/file.xml:ads", ContainerError::UnsafePath),
        ("word/file. ", ContainerError::UnsafePath),
        ("word\\evil.xml", ContainerError::UnsafePath),
        ("word/evil.exe", ContainerError::ForbiddenContent),
        ("word/vbaProject.bin", ContainerError::ForbiddenContent),
        ("word/embeddings/a.bin", ContainerError::ForbiddenContent),
        ("WORD/DOCUMENT.XML", ContainerError::DuplicatePath),
    ] {
        let mut p = parts(OfficeKind::Docx);
        p.push(Part {
            name: name.into(),
            body: b"<a/>".to_vec(),
        });
        assert_eq!(
            check(archive(&p, false, false), OfficeKind::Docx),
            Err(error),
            "{name}"
        );
    }
}

#[test]
fn zip_header_corruption_crc_symlink_encryption_overlap_and_truncation_fail() {
    let _guard = SERIAL.lock().unwrap();
    let data = archive(&parts(OfficeKind::Docx), true, true);
    let c = central(&data);
    for (at, bytes) in [
        (c + 8, vec![1, 8]),
        (c + 38, 0xa0000000u32.to_le_bytes().to_vec()),
        (c + 42, 1u32.to_le_bytes().to_vec()),
        (c + 24, (MAX_ENTRY as u32 + 1).to_le_bytes().to_vec()),
        (14, 1u32.to_le_bytes().to_vec()),
    ] {
        let mut changed = data.clone();
        changed[at..at + bytes.len()].copy_from_slice(&bytes);
        assert!(check(changed, OfficeKind::Docx).is_err(), "offset {at}");
    }
    let mut changed = data.clone();
    changed[c + 16] ^= 1;
    assert!(check(changed, OfficeKind::Docx).is_err());
    for len in [0, 21, data.len() - 1, c - 1] {
        assert!(check(data[..len].to_vec(), OfficeKind::Docx).is_err());
    }
    let mut prefixed = vec![0];
    prefixed.extend(&data);
    assert!(check(prefixed, OfficeKind::Docx).is_err());
}

#[test]
fn dtd_external_entities_unknown_entities_and_namespace_forgery_fail() {
    let _guard = SERIAL.lock().unwrap();
    for prefix in [
        "<!DOCTYPE a [<!ENTITY attack SYSTEM 'file:///private'>]>",
        "<!DOCTYPE a [<!ENTITY a 'a'>]>",
        "<?xml-stylesheet href='http://example.invalid/a'?>",
    ] {
        let mut p = parts(OfficeKind::Docx);
        p[2].body.splice(..0, prefix.bytes());
        assert!(check(archive(&p, true, false), OfficeKind::Docx).is_err());
    }
    let mut p = parts(OfficeKind::Docx);
    p[2].body = String::from_utf8(p[2].body.clone())
        .unwrap()
        .replace("<x:body/>", "<x:body>&unknown;</x:body>")
        .into_bytes();
    assert!(check(archive(&p, true, false), OfficeKind::Docx).is_err());
    let mut p = parts(OfficeKind::Docx);
    p[0].body = String::from_utf8(p[0].body.clone())
        .unwrap()
        .replace("<Override ", "<Override xmlns='urn:forged' ")
        .into_bytes();
    assert!(check(archive(&p, true, false), OfficeKind::Docx).is_err());
}

#[test]
fn relationships_resolve_inside_existing_parts_and_block_external_embeds() {
    let _guard = SERIAL.lock().unwrap();
    for target in [
        "../word/document.xml",
        "%2e%2e/word/document.xml",
        "https://example.invalid/a",
        "word/missing.xml",
        "/word/document.xml:ads",
    ] {
        let mut p = parts(OfficeKind::Docx);
        p[1].body = String::from_utf8(p[1].body.clone())
            .unwrap()
            .replace(
                "Target=\"word/document.xml\"",
                &format!("Target=\"{target}\""),
            )
            .into_bytes();
        assert!(
            check(archive(&p, true, false), OfficeKind::Docx).is_err(),
            "{target}"
        );
    }
    let mut p = parts(OfficeKind::Docx);
    let external="<Relationship Id='rId2' Type='http://schemas.openxmlformats.org/officeDocument/2006/relationships/image' TargetMode='External' Target='https://example.invalid/a.png'/>";
    p[1].body = String::from_utf8(p[1].body.clone())
        .unwrap()
        .replace("</Relationships>", &format!("{external}</Relationships>"))
        .into_bytes();
    assert_eq!(
        check(archive(&p, true, false), OfficeKind::Docx),
        Err(ContainerError::ForbiddenContent)
    );
}

#[test]
fn actual_xml_attribute_text_depth_and_global_event_limits_are_enforced() {
    let _guard = SERIAL.lock().unwrap();
    for inner in [
        format!("<x:body huge='{}'/>", "a".repeat(65537)),
        format!("<x:body>{}</x:body>", "a".repeat(1024 * 1024 + 1)),
        format!("{}{}", "<x:b>".repeat(65), "</x:b>".repeat(65)),
        "<x:b/>".repeat(250001),
    ] {
        let mut p = parts(OfficeKind::Docx);
        p[2].body = String::from_utf8(p[2].body.clone())
            .unwrap()
            .replace("<x:body/>", &inner)
            .into_bytes();
        assert_eq!(
            check(archive(&p, true, false), OfficeKind::Docx),
            Err(ContainerError::TooLarge)
        );
    }
}

#[test]
fn forged_expanded_size_cannot_hide_real_deflate_output() {
    let _guard = SERIAL.lock().unwrap();
    let mut p = parts(OfficeKind::Docx);
    p.push(Part {
        name: "word/media/a.png".into(),
        body: vec![0; MAX_ENTRY as usize + 1],
    });
    let mut data = archive(&p, true, true);
    let mut c = central(&data);
    for _ in 0..3 {
        c += 46 + usize::from(u16le(&data, c + 28));
    }
    data[c + 24..c + 28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(check(data, OfficeKind::Docx), Err(ContainerError::TooLarge));
}

#[test]
fn malformed_xml_names_characters_and_declarations_are_not_accepted_as_xml() {
    let _guard = SERIAL.lock().unwrap();
    for inner in [
        "<1bad/>",
        "<x:bad:alias/>",
        "<x:b attr='<invalid'/>",
        "<x:b unknown:attr='1'/>",
        "<x:b>]]></x:b>",
        "<!--\u{ffff}-->",
        "<?xml version='1.0'?>",
    ] {
        let mut p = parts(OfficeKind::Docx);
        p[2].body = String::from_utf8(p[2].body.clone())
            .unwrap()
            .replace("<x:body/>", inner)
            .into_bytes();
        assert!(
            check(archive(&p, true, false), OfficeKind::Docx).is_err(),
            "{inner}"
        );
    }
}

#[test]
fn uppercase_xml_extensions_cannot_bypass_the_xml_scanner() {
    let _guard = SERIAL.lock().unwrap();
    for name in [
        "word/other.XML",
        "word/media/other.SVG",
        "word/drawings/other.VML",
        "word/_rels/other.RELS",
    ] {
        let mut p = parts(OfficeKind::Docx);
        p.push(Part {
            name: name.into(),
            body: b"<!DOCTYPE x [<!ENTITY a 'evil'>]><x/>".to_vec(),
        });
        assert_eq!(
            check(archive(&p, true, false), OfficeKind::Docx),
            Err(ContainerError::ForbiddenContent)
        );
    }
}

#[test]
fn cancellation_actual_cpu_and_exclusive_slot_release_on_every_error() {
    let _guard = SERIAL.lock().unwrap();
    let data = archive(&parts(OfficeKind::Docx), true, false);
    struct Stop;
    impl ValidationControl for Stop {
        fn canceled(&self) -> bool {
            true
        }
        fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
            Ok(Duration::ZERO)
        }
    }
    assert_eq!(
        validate_office(&mut Cursor::new(&data), OfficeKind::Docx, &Stop),
        Err(ContainerError::Canceled)
    );
    struct Cpu(Cell<u32>);
    impl ValidationControl for Cpu {
        fn canceled(&self) -> bool {
            false
        }
        fn thread_cpu_time(&self) -> Result<Duration, ContainerError> {
            let n = self.0.get();
            self.0.set(n + 1);
            Ok(Duration::from_secs(u64::from(n) * 11))
        }
    }
    assert_eq!(
        validate_office(
            &mut Cursor::new(&data),
            OfficeKind::Docx,
            &Cpu(Cell::new(0))
        ),
        Err(ContainerError::CpuBudget)
    );
    let active = Exclusive::acquire().unwrap();
    assert_eq!(
        check(data.clone(), OfficeKind::Docx),
        Err(ContainerError::Busy)
    );
    drop(active);
    check(data, OfficeKind::Docx).unwrap();
}
