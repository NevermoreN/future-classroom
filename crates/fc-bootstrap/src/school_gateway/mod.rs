//! Optional, separately launched school certificate authority foundation.
//! This does not enroll classrooms or relay authenticated teacher traffic.
use pkcs8::EncodePrivateKey;
use rand::{rngs::OsRng, RngCore};
use rcgen::{
    BasicConstraints, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair,
    KeyUsagePurpose, PKCS_RSA_SHA256,
};
use rsa::RsaPrivateKey;
use rustls::pki_types::PrivatePkcs8KeyDer;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{error::Error, fs, io, net::IpAddr, path::Path};
use time::{Duration, OffsetDateTime};
pub mod server;
pub type Result<T> = std::result::Result<T, Box<dyn Error + Send + Sync>>;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SchoolConfig {
    pub schema: u32,
    pub school_id: String,
    pub school_name: String,
    pub published_ip: IpAddr,
    pub port: u16,
    pub leaf_expires_unix: i64,
    pub issuer_expires_unix: i64,
}

fn key_pair() -> Result<KeyPair> {
    let key = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let der = key.to_pkcs8_der()?;
    Ok(KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(der.as_bytes()),
        &PKCS_RSA_SHA256,
    )?)
}
fn params(name: &str, days: i64) -> Result<CertificateParams> {
    let mut p = CertificateParams::new(Vec::<String>::new())?;
    p.distinguished_name.push(DnType::CommonName, name);
    p.not_before = OffsetDateTime::now_utc() - Duration::minutes(5);
    p.not_after = OffsetDateTime::now_utc() + Duration::days(days);
    p.use_authority_key_identifier_extension = true;
    Ok(p)
}
fn encrypt(key: &KeyPair, password: &[u8]) -> Result<Vec<u8>> {
    let der = key.serialize_der();
    Ok(pkcs8::PrivateKeyInfo::try_from(der.as_slice())?
        .encrypt(&mut OsRng, password)?
        .as_bytes()
        .to_vec())
}
fn decrypt(bytes: &[u8], password: &[u8]) -> Result<KeyPair> {
    let der = pkcs8::EncryptedPrivateKeyInfo::try_from(bytes)?.decrypt(password)?;
    Ok(KeyPair::from_pkcs8_der_and_sign_algo(
        &PrivatePkcs8KeyDer::from(der.as_bytes()),
        &PKCS_RSA_SHA256,
    )?)
}
// Online state is additionally bound to the provisioning/service identity on
// Windows; offline recovery deliberately remains portable encrypted PKCS#8.
fn online_encrypt(key: &KeyPair, password: &[u8]) -> Result<Vec<u8>> {
    let encrypted = encrypt(key, password)?;
    #[cfg(windows)]
    {
        use fc_windows::system::{CurrentUserDpapi, SecretBytes};
        return Ok(CurrentUserDpapi::default().protect(&SecretBytes::new(encrypted)?)?);
    }
    #[cfg(not(windows))]
    {
        Ok(encrypted)
    }
}
fn online_decrypt(bytes: &[u8], password: &[u8]) -> Result<KeyPair> {
    #[cfg(windows)]
    {
        use fc_windows::system::CurrentUserDpapi;
        let decrypted = CurrentUserDpapi::default().unprotect(bytes)?;
        decrypt(decrypted.expose(), password)
    }
    #[cfg(not(windows))]
    {
        decrypt(bytes, password)
    }
}
fn create_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(path)?;
    }
    #[cfg(windows)]
    {
        fs::create_dir(path)?;
        let sid = fc_windows::system::current_user_sid()?;
        let status = std::process::Command::new("icacls.exe")
            .arg(path)
            .args([
                "/inheritance:r",
                "/grant:r",
                &format!("*{sid}:(OI)(CI)F"),
                "*S-1-5-18:(OI)(CI)F",
                "*S-1-5-32-544:(OI)(CI)F",
            ])
            .stdout(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            return Err("unable to restrict school state directory ACL".into());
        }
    }
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}
fn check_ip(ip: IpAddr) -> Result<()> {
    let allowed = match ip {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => ip.is_unique_local(),
    };
    if !allowed {
        return Err(
            "published address must be an explicit school-private IPv4 or IPv6 address".into(),
        );
    }
    Ok(())
}

/// Creates fresh school state and a separate encrypted root recovery directory.
/// The password must be supplied through stdin or a deployment secret, never argv.
pub fn initialize(
    state: &Path,
    recovery: &Path,
    name: &str,
    ip: IpAddr,
    port: u16,
    password: &[u8],
) -> Result<String> {
    check_ip(ip)?;
    if name.trim().is_empty()
        || name.chars().count() > 80
        || name.chars().any(char::is_control)
        || port == 0
        || password.len() < 16
    {
        return Err("school name, port, or password invalid (password minimum 16 bytes)".into());
    }
    // Both directories must be new. Never overwrite another school's identity.
    if state.exists() || recovery.exists() {
        return Err("state and offline recovery directories must both be new".into());
    }
    let state_parent = state
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let recovery_parent = recovery
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let state_path = state_parent.join(state.file_name().ok_or("invalid state path")?);
    let recovery_path = recovery_parent.join(recovery.file_name().ok_or("invalid recovery path")?);
    if recovery_path.starts_with(&state_path) || state_path.starts_with(&recovery_path) {
        return Err("offline recovery must be outside online school state".into());
    }
    let root_key = key_pair()?;
    let mut root_params = params(&format!("{name} Root CA"), 3650)?;
    root_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(1));
    root_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let root = root_params.self_signed(&root_key)?;
    let root_issuer = Issuer::from_params(&root_params, &root_key);
    let intermediate_key = key_pair()?;
    let mut intermediate_params = params(&format!("{name} Issuing CA"), 1095)?;
    intermediate_params.is_ca = IsCa::Ca(BasicConstraints::Constrained(0));
    intermediate_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let intermediate = intermediate_params.signed_by(&intermediate_key, &root_issuer)?;
    let mut identity = [0u8; 16];
    OsRng.fill_bytes(&mut identity);
    identity[6] = (identity[6] & 0x0f) | 0x40;
    identity[8] = (identity[8] & 0x3f) | 0x80;
    let mut config = SchoolConfig {
        schema: 1,
        school_id: uuid(&identity),
        school_name: name.to_owned(),
        published_ip: ip,
        port,
        leaf_expires_unix: 0,
        issuer_expires_unix: intermediate_params.not_after.unix_timestamp(),
    };
    let issuer = Issuer::from_params(&intermediate_params, &intermediate_key);
    let (leaf, leaf_key, expires) = issue_leaf(&config, &issuer)?;
    config.leaf_expires_unix = expires;
    // Generate and encrypt everything before creating any persistent state.
    let root_encrypted = encrypt(&root_key, password)?;
    let issuer_encrypted = online_encrypt(&intermediate_key, password)?;
    let leaf_encrypted = online_encrypt(&leaf_key, password)?;
    create_private_dir(recovery)?;
    write_new(&recovery.join("root-key.encrypted.pk8"), &root_encrypted)?;
    write_new(&recovery.join("root.pem"), root.pem().as_bytes())?;
    write_new(&recovery.join("README.txt"), b"Move this encrypted directory offline. Preserve the separate password. Online service must never retain the root private key.\n")?;
    create_private_dir(state)?;
    write_new(&state.join("root.pem"), root.pem().as_bytes())?;
    write_new(&state.join("root.cer"), root.der().as_ref())?;
    write_new(&state.join("issuer.pem"), intermediate.pem().as_bytes())?;
    write_new(&state.join("issuer-key.encrypted.pk8"), &issuer_encrypted)?;
    write_new(&state.join("server.pem"), leaf.pem().as_bytes())?;
    write_new(&state.join("server-key.encrypted.pk8"), &leaf_encrypted)?;
    write_new(
        &state.join("school.json"),
        &serde_json::to_vec_pretty(&config)?,
    )?;
    let fingerprint = hex(&Sha256::digest(root.der().as_ref()));
    write_new(&state.join("root-sha256.txt"), fingerprint.as_bytes())?;
    let certificate_data = root
        .pem()
        .lines()
        .filter(|line| !line.starts_with("-----"))
        .collect::<String>();
    let mut profile_identity = [0u8; 16];
    OsRng.fill_bytes(&mut profile_identity);
    profile_identity[6] = (profile_identity[6] & 0x0f) | 0x40;
    profile_identity[8] = (profile_identity[8] & 0x3f) | 0x80;
    let profile = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?><!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd"><plist version="1.0"><dict><key>PayloadType</key><string>Configuration</string><key>PayloadVersion</key><integer>1</integer><key>PayloadIdentifier</key><string>school.{}.trust</string><key>PayloadUUID</key><string>{}</string><key>PayloadDisplayName</key><string>Future Classroom School Certificate</string><key>PayloadContent</key><array><dict><key>PayloadType</key><string>com.apple.security.root</string><key>PayloadVersion</key><integer>1</integer><key>PayloadIdentifier</key><string>school.{}.root</string><key>PayloadUUID</key><string>{}</string><key>PayloadDisplayName</key><string>School Root CA</string><key>PayloadContent</key><data>{certificate_data}</data></dict></array></dict></plist>"#,
        config.school_id,
        config.school_id,
        config.school_id,
        uuid(&profile_identity)
    );
    write_new(&state.join("school-root.mobileconfig"), profile.as_bytes())?;
    write_new(&state.join("FIRST-USE.txt"), "管理员应通过已信任渠道分发 root.cer / school-root.mobileconfig 并线下核对 root-sha256.txt 指纹。iPhone 手动安装描述文件后，还需在设置→通用→关于本机→证书信任设置启用完整信任。Android 通过系统安装 CA 证书。网页不能自动完成系统信任，不要忽略证书警告输入账号密码。证书信任与 PWA 安装是不同步骤。\n".as_bytes())?;
    Ok(fingerprint)
}
fn issue_leaf(
    config: &SchoolConfig,
    issuer: &Issuer<'_, &KeyPair>,
) -> Result<(rcgen::Certificate, KeyPair, i64)> {
    let key = key_pair()?;
    let mut p = params(&config.school_name, 90)?;
    p.subject_alt_names = vec![rcgen::SanType::IpAddress(config.published_ip)];
    p.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    p.key_usages = vec![
        KeyUsagePurpose::DigitalSignature,
        KeyUsagePurpose::KeyEncipherment,
    ];
    Ok((
        p.signed_by(&key, issuer)?,
        key,
        p.not_after.unix_timestamp(),
    ))
}
fn uuid(bytes: &[u8; 16]) -> String {
    let raw = hex(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &raw[..8],
        &raw[8..12],
        &raw[12..16],
        &raw[16..20],
        &raw[20..]
    )
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Loads an existing encrypted identity. No implicit reinitialization or HTTP fallback.
pub fn load(state: &Path, password: &[u8]) -> Result<(SchoolConfig, rustls::ServerConfig)> {
    let config: SchoolConfig = serde_json::from_slice(&fs::read(state.join("school.json"))?)?;
    if config.schema != 1 || config.port == 0 {
        return Err("unsupported school state".into());
    }
    check_ip(config.published_ip)?;
    if config.leaf_expires_unix <= OffsetDateTime::now_utc().unix_timestamp() {
        return Err("gateway certificate expired; explicit certificate renewal is required".into());
    }
    let leaf_pem = fs::read_to_string(state.join("server.pem"))?;
    let issuer_pem = fs::read_to_string(state.join("issuer.pem"))?;
    let key = online_decrypt(&fs::read(state.join("server-key.encrypted.pk8"))?, password)?;
    use rustls::pki_types::{pem::PemObject, CertificateDer};
    let leaf = CertificateDer::from_pem_slice(leaf_pem.as_bytes())?;
    let issuer = CertificateDer::from_pem_slice(issuer_pem.as_bytes())?;
    use rustls::client::danger::ServerCertVerifier;
    let mut roots = rustls::RootCertStore::empty();
    roots.add(CertificateDer::from_pem_slice(&fs::read(
        state.join("root.pem"),
    )?)?)?;
    rustls::client::WebPkiServerVerifier::builder_with_provider(
        std::sync::Arc::new(roots),
        std::sync::Arc::new(rustls::crypto::ring::default_provider()),
    )
    .build()?
    .verify_server_cert(
        &leaf,
        &[issuer.clone()],
        &config.published_ip.into(),
        &[],
        rustls::pki_types::UnixTime::now(),
    )?;
    let tls = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_protocol_versions(&[&rustls::version::TLS13, &rustls::version::TLS12])?
    .with_no_client_auth()
    .with_single_cert(
        vec![leaf, issuer],
        PrivatePkcs8KeyDer::from(key.serialize_der()).into(),
    )?;
    Ok((config, tls))
}

/// Issues and validates a fresh leaf in a NEW candidate state directory.
/// Original files are never modified. Promotion/restart must be explicit until
/// the service manager and transactional activation have been implemented.
pub fn renew_candidate(state: &Path, candidate: &Path, password: &[u8]) -> Result<()> {
    let mut config: SchoolConfig = serde_json::from_slice(&fs::read(state.join("school.json"))?)?;
    if config.schema != 1 || config.port == 0 {
        return Err("unsupported school state".into());
    }
    check_ip(config.published_ip)?;
    if config.issuer_expires_unix
        <= (OffsetDateTime::now_utc() + Duration::days(90)).unix_timestamp()
    {
        return Err(
            "issuing CA requires offline-root renewal before another 90-day leaf can be issued"
                .into(),
        );
    }
    let issuer_key = online_decrypt(&fs::read(state.join("issuer-key.encrypted.pk8"))?, password)?;
    let issuer =
        Issuer::from_ca_cert_pem(&fs::read_to_string(state.join("issuer.pem"))?, &issuer_key)?;
    let (leaf, key, expires) = issue_leaf(&config, &issuer)?;
    config.leaf_expires_unix = expires;
    let encrypted_key = online_encrypt(&key, password)?;
    create_private_dir(candidate)?;
    for name in [
        "root.pem",
        "root.cer",
        "issuer.pem",
        "issuer-key.encrypted.pk8",
        "root-sha256.txt",
        "school-root.mobileconfig",
        "FIRST-USE.txt",
    ] {
        write_new(&candidate.join(name), &fs::read(state.join(name))?)?;
    }
    write_new(&candidate.join("server.pem"), leaf.pem().as_bytes())?;
    write_new(&candidate.join("server-key.encrypted.pk8"), &encrypted_key)?;
    write_new(
        &candidate.join("school.json"),
        &serde_json::to_vec_pretty(&config)?,
    )?;
    let _ = load(candidate, password)?;
    Ok(())
}

/// Reads a single password line without putting the secret in command history or argv.
pub fn read_password() -> Result<Vec<u8>> {
    use io::{BufRead, Read};
    let mut password = Vec::new();
    io::stdin()
        .lock()
        .take(4097)
        .read_until(b'\n', &mut password)?;
    if password.last() == Some(&b'\n') {
        password.pop();
    }
    if password.last() == Some(&b'\r') {
        password.pop();
    }
    if !(16..=4096).contains(&password.len()) {
        return Err("provide one password line of 16..4096 bytes on stdin".into());
    }
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn school_chain_encryption_and_explicit_renewal_preserve_identity() {
        let mut nonce = [0u8; 16];
        OsRng.fill_bytes(&mut nonce);
        let base = std::env::temp_dir().join(format!("fc-school-ca-{}", hex(&nonce)));
        fs::create_dir(&base).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(base.clone());
        let online = base.join("online");
        let offline = base.join("offline");
        let password = b"a test-only strong recovery secret";
        let fingerprint = initialize(
            &online,
            &offline,
            "School Test",
            "192.168.42.7".parse().unwrap(),
            9443,
            password,
        )
        .unwrap();
        assert_eq!(fingerprint.len(), 64);
        assert!(offline.join("root-key.encrypted.pk8").exists());
        assert!(!online.join("root-key.encrypted.pk8").exists());
        assert!(load(&online, b"wrong-test-only-password").is_err());
        let (config, tls) = load(&online, password).unwrap();
        server::test_https_directory(&config, tls, &online);
        let root = decrypt(
            &fs::read(offline.join("root-key.encrypted.pk8")).unwrap(),
            password,
        )
        .unwrap();
        assert_eq!(root.algorithm(), &PKCS_RSA_SHA256);
        assert!(initialize(
            &online,
            &offline,
            "Other School",
            "192.168.42.8".parse().unwrap(),
            9443,
            password
        )
        .is_err());
        let original = fs::read(online.join("server.pem")).unwrap();
        let candidate = base.join("candidate");
        renew_candidate(&online, &candidate, password).unwrap();
        let (renewed, _) = load(&candidate, password).unwrap();
        assert_eq!(config.school_id, renewed.school_id);
        assert_eq!(
            fs::read(online.join("root.cer")).unwrap(),
            fs::read(candidate.join("root.cer")).unwrap()
        );
        assert_eq!(original, fs::read(online.join("server.pem")).unwrap());
        assert_ne!(original, fs::read(candidate.join("server.pem")).unwrap());
        let mut changed = renewed;
        changed.published_ip = "192.168.42.8".parse().unwrap();
        fs::write(
            candidate.join("school.json"),
            serde_json::to_vec(&changed).unwrap(),
        )
        .unwrap();
        assert!(
            load(&candidate, password).is_err(),
            "certificate must reject a changed IP SAN"
        );
    }
}
