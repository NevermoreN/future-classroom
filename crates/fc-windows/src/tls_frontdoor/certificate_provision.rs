//! Per-installation IP certificates and explicit CurrentUser-only owned trust.
use std::net::IpAddr;
#[cfg(windows)]
mod local_hosts;

/// Assigned local unicast addresses only. This grants no peer/CIDR authority.
pub fn local_certificate_hosts() -> Result<Vec<String>, CertificateProvisionError> {
    #[cfg(windows)]
    {
        local_hosts::enumerate()
    }
    #[cfg(not(windows))]
    {
        Err(CertificateProvisionError::Unavailable)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProvisionedCertificate {
    pub thumbprint: String,
    pub public_der: Vec<u8>,
    pub reused: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CertificateProvisionError {
    InvalidIp,
    Cancelled,
    Unavailable,
    Native(i32),
    Cleanup(i32),
}

pub fn provision_ip_certificate(
    host: &str,
    active: impl FnMut() -> bool,
) -> Result<ProvisionedCertificate, CertificateProvisionError> {
    provision_hosts_certificate(&[host], active)
}
pub fn provision_hosts_certificate(
    hosts: &[&str],
    mut active: impl FnMut() -> bool,
) -> Result<ProvisionedCertificate, CertificateProvisionError> {
    if hosts.is_empty() || hosts.len() > 16 {
        return Err(CertificateProvisionError::InvalidIp);
    }
    let mut ips = hosts
        .iter()
        .map(|host| {
            host.parse::<IpAddr>()
                .map_err(|_| CertificateProvisionError::InvalidIp)
        })
        .collect::<Result<Vec<_>, _>>()?;
    if ips
        .iter()
        .any(|ip| ip.is_unspecified() || ip.is_multicast())
    {
        return Err(CertificateProvisionError::InvalidIp);
    }
    ips.sort_unstable();
    ips.dedup();
    if !active() {
        return Err(CertificateProvisionError::Cancelled);
    }
    #[cfg(windows)]
    {
        native::provision(&ips, &mut active)
    }
    #[cfg(not(windows))]
    {
        let _ = ips;
        Err(CertificateProvisionError::Unavailable)
    }
}

#[cfg(test)]
fn san_der(ip: IpAddr) -> Vec<u8> {
    hosts_san_der(&[ip])
}
#[cfg(any(windows, test))]
fn hosts_san_der(ips: &[IpAddr]) -> Vec<u8> {
    let mut content = Vec::new();
    for ip in ips {
        let bytes = match ip {
            IpAddr::V4(ip) => ip.octets().to_vec(),
            IpAddr::V6(ip) => ip.octets().to_vec(),
        };
        content.extend([0x87, bytes.len() as u8]);
        content.extend(bytes);
    }
    let mut der = if content.len() < 128 {
        vec![0x30, content.len() as u8]
    } else if content.len() <= 255 {
        vec![0x30, 0x81, content.len() as u8]
    } else {
        vec![0x30, 0x82, (content.len() >> 8) as u8, content.len() as u8]
    };
    der.extend(content);
    der
}
pub fn export_owned_certificate(
    thumbprint: &str,
) -> Result<ProvisionedCertificate, CertificateProvisionError> {
    #[cfg(windows)]
    {
        native::export_owned(thumbprint)
    }
    #[cfg(not(windows))]
    {
        let _ = thumbprint;
        Err(CertificateProvisionError::Unavailable)
    }
}

pub fn trust_owned_certificate(thumbprint: &str) -> Result<(), CertificateProvisionError> {
    #[cfg(windows)]
    {
        native::trust_owned(thumbprint)
    }
    #[cfg(not(windows))]
    {
        let _ = thumbprint;
        Err(CertificateProvisionError::Unavailable)
    }
}

#[cfg(windows)]
mod native {
    use super::*;
    use windows::{
        core::{PCWSTR, PWSTR},
        Win32::{
            Foundation::CRYPT_E_NOT_FOUND, Security::Cryptography::*,
            System::SystemInformation::GetSystemTime,
        },
    };
    const PREFIX: &str = "FutureClassroom-IP-v1-";
    const EKU: &[u8] = &[0x30, 10, 6, 8, 0x2b, 6, 1, 5, 5, 7, 3, 1];
    const NON_CA: &[u8] = &[0x30, 0];
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }
    fn native(e: windows::core::Error) -> CertificateProvisionError {
        CertificateProvisionError::Native(e.code().0)
    }
    fn last() -> CertificateProvisionError {
        native(windows::core::Error::from_thread())
    }
    fn check(active: &mut impl FnMut() -> bool) -> Result<(), CertificateProvisionError> {
        if active() {
            Ok(())
        } else {
            Err(CertificateProvisionError::Cancelled)
        }
    }
    struct Owner {
        store: Option<HCERTSTORE>,
        cert: *mut CERT_CONTEXT,
        provider: usize,
        key: usize,
        container: Vec<u16>,
        rollback: bool,
        stored: bool,
    }
    impl Owner {
        fn close(&mut self) -> Result<(), CertificateProvisionError> {
            let mut failure = None;
            let mut record = |r: windows::core::Result<()>| {
                if let Err(e) = r {
                    failure.get_or_insert(CertificateProvisionError::Cleanup(e.code().0));
                }
            };
            // SAFETY: each native owner is consumed once on this synchronous worker.
            unsafe {
                if self.key != 0 {
                    record(CryptDestroyKey(std::mem::take(&mut self.key)));
                }
                if self.provider != 0 {
                    record(CryptReleaseContext(std::mem::take(&mut self.provider), 0));
                }
                if !self.cert.is_null() {
                    let cert = self.cert;
                    self.cert = std::ptr::null_mut();
                    if self.rollback && self.stored {
                        record(CertDeleteCertificateFromStore(cert));
                    } else {
                        record(CertFreeCertificateContext(Some(cert)).ok());
                    }
                }
                if self.rollback && !self.container.is_empty() {
                    let mut ignored = 0;
                    record(CryptAcquireContextW(
                        &mut ignored,
                        PCWSTR(self.container.as_ptr()),
                        MS_ENH_RSA_AES_PROV_W,
                        PROV_RSA_AES,
                        CRYPT_DELETEKEYSET,
                    ));
                    self.container.clear();
                }
                if let Some(store) = self.store.take() {
                    record(CertCloseStore(Some(store), CERT_CLOSE_STORE_CHECK_FLAG));
                }
            }
            failure.map_or(Ok(()), Err)
        }
    }
    impl Drop for Owner {
        fn drop(&mut self) {
            let _ = self.close();
        }
    }
    fn property(cert: *const CERT_CONTEXT, id: u32) -> Result<Vec<u8>, CertificateProvisionError> {
        let mut len = 0;
        // SAFETY: live owned certificate and native-sized property output.
        unsafe { CertGetCertificateContextProperty(cert, id, None, &mut len) }.map_err(native)?;
        if len > 16384 {
            return Err(CertificateProvisionError::Unavailable);
        }
        let mut bytes = vec![0; len as usize];
        unsafe {
            CertGetCertificateContextProperty(cert, id, Some(bytes.as_mut_ptr().cast()), &mut len)
        }
        .map_err(native)?;
        bytes.truncate(len as usize);
        Ok(bytes)
    }
    fn result(
        cert: *const CERT_CONTEXT,
        reused: bool,
    ) -> Result<ProvisionedCertificate, CertificateProvisionError> {
        let hash = property(cert, CERT_SHA1_HASH_PROP_ID)?;
        if hash.len() != 20 {
            return Err(CertificateProvisionError::Unavailable);
        }
        // SAFETY: context remains owned while its public DER is copied.
        let public_der = unsafe {
            std::slice::from_raw_parts((*cert).pbCertEncoded, (*cert).cbCertEncoded as usize)
        }
        .to_vec();
        Ok(ProvisionedCertificate {
            thumbprint: hash.iter().map(|b| format!("{b:02x}")).collect(),
            public_der,
            reused,
        })
    }
    fn owned_container(cert: *const CERT_CONTEXT) -> Result<Vec<u16>, CertificateProvisionError> {
        // Property contains embedded pointers into its native-aligned buffer.
        let mut len = 0;
        unsafe {
            CertGetCertificateContextProperty(cert, CERT_KEY_PROV_INFO_PROP_ID, None, &mut len)
        }
        .map_err(native)?;
        if len as usize > 16384 || (len as usize) < std::mem::size_of::<CRYPT_KEY_PROV_INFO>() {
            return Err(CertificateProvisionError::Unavailable);
        }
        let mut buf = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
        unsafe {
            CertGetCertificateContextProperty(
                cert,
                CERT_KEY_PROV_INFO_PROP_ID,
                Some(buf.as_mut_ptr().cast()),
                &mut len,
            )
        }
        .map_err(native)?;
        let info = unsafe { &*buf.as_ptr().cast::<CRYPT_KEY_PROV_INFO>() };
        if info.dwProvType != PROV_RSA_AES
            || info.dwKeySpec != AT_KEYEXCHANGE.0
            || info.dwFlags.0 & CRYPT_MACHINE_KEYSET.0 != 0
        {
            return Err(CertificateProvisionError::Unavailable);
        }
        let name = unsafe { info.pwszContainerName.to_string() }
            .map_err(|_| CertificateProvisionError::Unavailable)?;
        if !name.starts_with(PREFIX)
            || name.len() != PREFIX.len() + 32
            || !name[PREFIX.len()..].bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(CertificateProvisionError::Unavailable);
        }
        Ok(wide(&name))
    }
    fn matches(cert: *const CERT_CONTEXT, label: &[u16], san: &[u8]) -> bool {
        let expected: Vec<u8> = label.iter().flat_map(|v| v.to_le_bytes()).collect();
        if property(cert, CERT_FRIENDLY_NAME_PROP_ID).ok().as_deref() != Some(expected.as_slice())
            || owned_container(cert).is_err()
        {
            return false;
        }
        // SAFETY: certificate info and extension storage belong to the context.
        unsafe {
            let info = &*(*cert).pCertInfo;
            if CertVerifyTimeValidity(None, info) != 0 {
                return false;
            }
            let extensions = std::slice::from_raw_parts(info.rgExtension, info.cExtension as usize);
            [
                (szOID_SUBJECT_ALT_NAME2, san),
                (szOID_ENHANCED_KEY_USAGE, EKU),
                (szOID_BASIC_CONSTRAINTS2, NON_CA),
            ]
            .iter()
            .all(|(oid, der)| {
                extensions.iter().any(|ext| {
                    ext.pszObjId.as_bytes() == oid.as_bytes()
                        && std::slice::from_raw_parts(ext.Value.pbData, ext.Value.cbData as usize)
                            == *der
                })
            })
        }
    }
    fn owned_public(
        certificate: *const CERT_CONTEXT,
    ) -> Result<ProvisionedCertificate, CertificateProvisionError> {
        owned_container(certificate)?;
        let raw_label = property(certificate, CERT_FRIENDLY_NAME_PROP_ID)?;
        let label = String::from_utf16(
            &raw_label
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| CertificateProvisionError::Unavailable)?;
        let label = label.trim_end_matches('\0');
        let identities = label
            .strip_prefix(PREFIX)
            .ok_or(CertificateProvisionError::Unavailable)?;
        let ips = identities
            .split(',')
            .map(|v| {
                v.parse::<IpAddr>()
                    .map_err(|_| CertificateProvisionError::Unavailable)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if ips.is_empty()
            || ips.len() > 16
            || !matches(certificate, &wide(label), &hosts_san_der(&ips))
        {
            return Err(CertificateProvisionError::Unavailable);
        }
        result(certificate, true)
    }
    pub(super) fn export_owned(
        thumbprint: &str,
    ) -> Result<ProvisionedCertificate, CertificateProvisionError> {
        let hash = super::super::CertificateThumbprint::parse(thumbprint)
            .map_err(|_| CertificateProvisionError::Unavailable)?;
        let mut selected =
            super::super::certificate_selection::SelectedCertificate::open().map_err(native)?;
        let outcome = (|| {
            if !selected.find(hash).map_err(native)? {
                return Err(CertificateProvisionError::Unavailable);
            }
            owned_public(selected.certificate())
        })();
        selected
            .close(true)
            .map_err(|_| CertificateProvisionError::Unavailable)?;
        outcome
    }
    pub(super) fn trust_owned(thumbprint: &str) -> Result<(), CertificateProvisionError> {
        let hash = super::super::CertificateThumbprint::parse(thumbprint)
            .map_err(|_| CertificateProvisionError::Unavailable)?;
        let mut selected =
            super::super::certificate_selection::SelectedCertificate::open().map_err(native)?;
        let mut root = Owner {
            store: None,
            cert: std::ptr::null_mut(),
            provider: 0,
            key: 0,
            container: Vec::new(),
            rollback: false,
            stored: false,
        };
        let outcome = (|| {
            if !selected.find(hash).map_err(native)? {
                return Err(CertificateProvisionError::Unavailable);
            }
            let certificate = selected.certificate();
            let public = owned_public(certificate)?;
            // SAFETY: only the current user's ROOT store is opened. DER copying
            // deliberately excludes the MY private-key/container properties.
            root.store = Some(
                unsafe {
                    CertOpenStore(
                        CERT_STORE_PROV_SYSTEM_W,
                        CERT_QUERY_ENCODING_TYPE(0),
                        None,
                        CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER),
                        Some(windows::core::w!("ROOT").as_ptr().cast()),
                    )
                }
                .map_err(native)?,
            );
            unsafe {
                CertAddEncodedCertificateToStore(
                    root.store,
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    &public.public_der,
                    CERT_STORE_ADD_USE_EXISTING,
                    None,
                )
            }
            .map_err(native)?;
            let blob = CRYPT_INTEGER_BLOB {
                cbData: 20,
                pbData: hash.bytes().as_ptr().cast_mut(),
            };
            root.cert = unsafe {
                CertFindCertificateInStore(
                    root.store.unwrap(),
                    X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                    0,
                    CERT_FIND_HASH,
                    Some((&blob as *const CRYPT_INTEGER_BLOB).cast()),
                    None,
                )
            };
            if root.cert.is_null() || result(root.cert, true)?.public_der != public.public_der {
                return Err(CertificateProvisionError::Unavailable);
            }
            Ok(())
        })();
        let root_cleanup = root.close();
        let source_cleanup = selected
            .close(true)
            .map_err(|_| CertificateProvisionError::Unavailable);
        root_cleanup?;
        source_cleanup?;
        outcome
    }
    pub(super) fn provision(
        ips: &[IpAddr],
        active: &mut impl FnMut() -> bool,
    ) -> Result<ProvisionedCertificate, CertificateProvisionError> {
        let identity = ips
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let label = wide(&format!("{PREFIX}{identity}"));
        let san = hosts_san_der(ips);
        let mut owner = Owner {
            store: None,
            cert: std::ptr::null_mut(),
            provider: 0,
            key: 0,
            container: Vec::new(),
            rollback: false,
            stored: false,
        };
        let outcome = (|| {
            // SAFETY: explicit current-user MY only; never ROOT or machine stores.
            owner.store = Some(
                unsafe {
                    CertOpenStore(
                        CERT_STORE_PROV_SYSTEM_W,
                        CERT_QUERY_ENCODING_TYPE(0),
                        None,
                        CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER),
                        Some(windows::core::w!("MY").as_ptr().cast()),
                    )
                }
                .map_err(native)?,
            );
            loop {
                check(active)?;
                owner.cert = unsafe {
                    CertEnumCertificatesInStore(
                        owner.store.unwrap(),
                        if owner.cert.is_null() {
                            None
                        } else {
                            Some(owner.cert)
                        },
                    )
                };
                if owner.cert.is_null() {
                    let e = windows::core::Error::from_thread();
                    if e.code() != CRYPT_E_NOT_FOUND {
                        return Err(native(e));
                    }
                    break;
                }
                if matches(owner.cert, &label, &san) {
                    let mut selected =
                        super::super::certificate_selection::SelectedCertificate::open()
                            .map_err(native)?;
                    let value = result(owner.cert, true)?;
                    let thumb = super::super::CertificateThumbprint::parse(&value.thumbprint)
                        .map_err(|_| CertificateProvisionError::Unavailable)?;
                    if selected.find(thumb).map_err(native)?
                        && selected.acquire_private_key().is_ok()
                    {
                        selected
                            .close(true)
                            .map_err(|_| CertificateProvisionError::Unavailable)?;
                        check(active)?;
                        return Ok(value);
                    }
                }
            }
            check(active)?;
            // Generate an unpredictable per-certificate container name without any exported key.
            unsafe {
                CryptAcquireContextW(
                    &mut owner.provider,
                    PCWSTR::null(),
                    MS_ENH_RSA_AES_PROV_W,
                    PROV_RSA_AES,
                    CRYPT_VERIFYCONTEXT,
                )
            }
            .map_err(native)?;
            let mut nonce = [0; 16];
            unsafe { CryptGenRandom(owner.provider, &mut nonce) }.map_err(native)?;
            unsafe { CryptReleaseContext(std::mem::take(&mut owner.provider), 0) }
                .map_err(native)?;
            let name = format!(
                "{PREFIX}{}",
                nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
            );
            let container = wide(&name);
            unsafe {
                CryptAcquireContextW(
                    &mut owner.provider,
                    PCWSTR(container.as_ptr()),
                    MS_ENH_RSA_AES_PROV_W,
                    PROV_RSA_AES,
                    CRYPT_NEWKEYSET | CRYPT_SILENT,
                )
            }
            .map_err(native)?;
            owner.container = container;
            owner.rollback = true;
            check(active)?;
            // No CRYPT_EXPORTABLE flag: the persisted RSA private key is non-exportable.
            unsafe {
                CryptGenKey(
                    owner.provider,
                    ALG_ID(AT_KEYEXCHANGE.0),
                    CRYPT_KEY_FLAGS(2048 << 16),
                    &mut owner.key,
                )
            }
            .map_err(native)?;
            check(active)?;
            let subject = wide(&format!("CN=FutureClassroom {}", ips[0]));
            let mut subject_len = 0;
            unsafe {
                CertStrToNameW(
                    X509_ASN_ENCODING,
                    PCWSTR(subject.as_ptr()),
                    CERT_X500_NAME_STR,
                    None,
                    None,
                    &mut subject_len,
                    None,
                )
            }
            .map_err(native)?;
            let mut subject_der = vec![0; subject_len as usize];
            unsafe {
                CertStrToNameW(
                    X509_ASN_ENCODING,
                    PCWSTR(subject.as_ptr()),
                    CERT_X500_NAME_STR,
                    None,
                    Some(subject_der.as_mut_ptr()),
                    &mut subject_len,
                    None,
                )
            }
            .map_err(native)?;
            let subject_blob = CRYPT_INTEGER_BLOB {
                cbData: subject_len,
                pbData: subject_der.as_mut_ptr(),
            };
            let provider = CRYPT_KEY_PROV_INFO {
                pwszContainerName: PWSTR(owner.container.as_mut_ptr()),
                pwszProvName: PWSTR(MS_ENH_RSA_AES_PROV_W.0.cast_mut()),
                dwProvType: PROV_RSA_AES,
                dwKeySpec: AT_KEYEXCHANGE.0,
                ..Default::default()
            };
            let algorithm = CRYPT_ALGORITHM_IDENTIFIER {
                pszObjId: windows::core::PSTR(szOID_RSA_SHA256RSA.0.cast_mut()),
                ..Default::default()
            };
            let mut extensions = [
                (szOID_SUBJECT_ALT_NAME2, san.as_slice(), false),
                (szOID_ENHANCED_KEY_USAGE, EKU, false),
                (szOID_BASIC_CONSTRAINTS2, NON_CA, true),
            ]
            .map(|(oid, data, critical)| CERT_EXTENSION {
                pszObjId: windows::core::PSTR(oid.0.cast_mut()),
                fCritical: critical.into(),
                Value: CRYPT_INTEGER_BLOB {
                    cbData: data.len() as u32,
                    pbData: data.as_ptr().cast_mut(),
                },
            });
            let extensions = CERT_EXTENSIONS {
                cExtension: extensions.len() as u32,
                rgExtension: extensions.as_mut_ptr(),
            };
            let start = unsafe { GetSystemTime() };
            let mut end = start;
            end.wYear += 1;
            if end.wMonth == 2 && end.wDay == 29 {
                end.wDay = 28;
            }
            owner.cert = unsafe {
                CertCreateSelfSignCertificate(
                    Some(HCRYPTPROV_OR_NCRYPT_KEY_HANDLE(owner.provider)),
                    &subject_blob,
                    CERT_CREATE_SELFSIGN_FLAGS(0),
                    Some(&provider),
                    Some(&algorithm),
                    Some(&start),
                    Some(&end),
                    Some(&extensions),
                )
            };
            if owner.cert.is_null() {
                return Err(last());
            }
            let label_blob = CRYPT_INTEGER_BLOB {
                cbData: (label.len() * 2) as u32,
                pbData: label.as_ptr().cast_mut().cast(),
            };
            unsafe {
                CertSetCertificateContextProperty(
                    owner.cert,
                    CERT_FRIENDLY_NAME_PROP_ID,
                    0,
                    Some((&label_blob as *const CRYPT_INTEGER_BLOB).cast()),
                )
            }
            .map_err(native)?;
            check(active)?;
            let mut stored = std::ptr::null_mut();
            unsafe {
                CertAddCertificateContextToStore(
                    owner.store,
                    owner.cert,
                    CERT_STORE_ADD_NEW,
                    Some(&mut stored),
                )
            }
            .map_err(native)?;
            unsafe { CertFreeCertificateContext(Some(owner.cert)) }
                .ok()
                .map_err(native)?;
            owner.cert = stored;
            owner.stored = true;
            check(active)?;
            result(owner.cert, false)
        })();
        if outcome.is_ok() {
            owner.rollback = false;
        }
        let cleanup = owner.close();
        cleanup?;
        outcome
    }

    #[cfg(test)]
    pub(super) fn verify_private_key_not_exportable(
        thumbprint: &str,
    ) -> Result<(), CertificateProvisionError> {
        let hash = super::super::CertificateThumbprint::parse(thumbprint)
            .map_err(|_| CertificateProvisionError::InvalidIp)?;
        let mut selected =
            super::super::certificate_selection::SelectedCertificate::open().map_err(native)?;
        if !selected.find(hash).map_err(native)? {
            return Err(CertificateProvisionError::Unavailable);
        }
        let container = owned_container(selected.certificate())?;
        let mut owner = Owner {
            store: None,
            cert: std::ptr::null_mut(),
            provider: 0,
            key: 0,
            container: Vec::new(),
            rollback: false,
            stored: false,
        };
        // SAFETY: open only the selected feature-owned existing user container.
        unsafe {
            CryptAcquireContextW(
                &mut owner.provider,
                PCWSTR(container.as_ptr()),
                MS_ENH_RSA_AES_PROV_W,
                PROV_RSA_AES,
                CRYPT_SILENT,
            )
        }
        .map_err(native)?;
        unsafe { CryptGetUserKey(owner.provider, AT_KEYEXCHANGE.0, &mut owner.key) }
            .map_err(native)?;
        let mut length = 0;
        // Length-only query: no private-key buffer is allocated or supplied.
        let export = unsafe {
            CryptExportKey(
                owner.key,
                0,
                PRIVATEKEYBLOB,
                CRYPT_KEY_FLAGS(0),
                None,
                &mut length,
            )
        };
        owner.close()?;
        selected
            .close(true)
            .map_err(|_| CertificateProvisionError::Unavailable)?;
        match export {
            Err(error) if error.code() == windows::Win32::Foundation::NTE_BAD_KEY_STATE => Ok(()),
            Err(error) => Err(native(error)),
            Ok(()) => Err(CertificateProvisionError::Unavailable),
        }
    }

    /// Only for an explicitly selected test-created certificate. Never matches legacy deployment labels.
    #[cfg(test)]
    pub fn remove_test_certificate(thumbprint: &str) -> Result<(), CertificateProvisionError> {
        let hash = super::super::CertificateThumbprint::parse(thumbprint)
            .map_err(|_| CertificateProvisionError::InvalidIp)?;
        let mut owner = Owner {
            store: None,
            cert: std::ptr::null_mut(),
            provider: 0,
            key: 0,
            container: Vec::new(),
            rollback: false,
            stored: false,
        };
        owner.store = Some(
            unsafe {
                CertOpenStore(
                    CERT_STORE_PROV_SYSTEM_W,
                    CERT_QUERY_ENCODING_TYPE(0),
                    None,
                    CERT_OPEN_STORE_FLAGS(CERT_SYSTEM_STORE_CURRENT_USER)
                        | CERT_STORE_OPEN_EXISTING_FLAG,
                    Some(windows::core::w!("MY").as_ptr().cast()),
                )
            }
            .map_err(native)?,
        );
        let blob = CRYPT_INTEGER_BLOB {
            cbData: 20,
            pbData: hash.bytes().as_ptr().cast_mut(),
        };
        owner.cert = unsafe {
            CertFindCertificateInStore(
                owner.store.unwrap(),
                X509_ASN_ENCODING | PKCS_7_ASN_ENCODING,
                0,
                CERT_FIND_HASH,
                Some((&blob as *const CRYPT_INTEGER_BLOB).cast()),
                None,
            )
        };
        if owner.cert.is_null() {
            let error = windows::core::Error::from_thread();
            return if error.code() == CRYPT_E_NOT_FOUND {
                owner.close()
            } else {
                Err(native(error))
            };
        }
        let label = property(owner.cert, CERT_FRIENDLY_NAME_PROP_ID)?;
        let label = String::from_utf16(
            &label
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(|_| CertificateProvisionError::Unavailable)?;
        if !label.starts_with(PREFIX) {
            return Err(CertificateProvisionError::Unavailable);
        }
        owner.container = owned_container(owner.cert)?;
        owner.rollback = true;
        owner.stored = true;
        owner.close()
    }
}

#[cfg(all(test, windows))]
pub use native::remove_test_certificate;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multi_host_san_has_exact_ip_entries_and_bounded_input() {
        assert_eq!(
            hosts_san_der(&["127.0.0.1".parse().unwrap(), "192.0.2.1".parse().unwrap()]),
            [0x30, 12, 0x87, 4, 127, 0, 0, 1, 0x87, 4, 192, 0, 2, 1]
        );
        for hosts in [
            vec![],
            vec!["127.0.0.1"; 17],
            vec!["127.0.0.1", "bad"],
            vec!["0.0.0.0"],
        ] {
            assert_eq!(
                provision_hosts_certificate(&hosts, || true),
                Err(CertificateProvisionError::InvalidIp)
            );
        }
        assert_eq!(
            provision_hosts_certificate(&["127.0.0.1", "192.0.2.1"], || false),
            Err(CertificateProvisionError::Cancelled)
        );
        let v6 = (1..=16)
            .map(|n| format!("2001:db8::{n:x}").parse().unwrap())
            .collect::<Vec<IpAddr>>();
        let der = hosts_san_der(&v6);
        assert_eq!(&der[..4], &[0x30, 0x82, 1, 32]);
        assert_eq!(der.len(), 292);
    }
    #[cfg(windows)]
    #[test]
    #[ignore = "creates a scoped CurrentUser MY certificate; explicit Win11 run only"]
    fn native_ip_certificate_create_reuse_and_cleanup() {
        let first = provision_ip_certificate("192.0.2.123", || true).unwrap();
        struct Cleanup(Option<String>);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if let Some(hash) = &self.0 {
                    remove_test_certificate(hash).unwrap();
                }
            }
        }
        let _cleanup = Cleanup((!first.reused).then(|| first.thumbprint.clone()));
        assert_eq!(first.thumbprint.len(), 40);
        assert!(first.public_der.len() > 256);
        let second = provision_ip_certificate("192.0.2.123", || true).unwrap();
        assert!(second.reused);
        assert_eq!(first.thumbprint, second.thumbprint);
        assert_eq!(first.public_der, second.public_der);
        native::verify_private_key_not_exportable(&first.thumbprint).unwrap();
    }
    #[test]
    fn exact_ip_san_der_and_invalid_input() {
        assert_eq!(
            san_der("192.0.2.1".parse().unwrap()),
            [0x30, 6, 0x87, 4, 192, 0, 2, 1]
        );
        let v6 = san_der("2001:db8::1".parse().unwrap());
        assert_eq!(&v6[..4], &[0x30, 18, 0x87, 16]);
        assert_eq!(v6.len(), 20);
        for ip in ["example.org", "0.0.0.0", "224.0.0.1", "::"] {
            assert_eq!(
                provision_ip_certificate(ip, || true),
                Err(CertificateProvisionError::InvalidIp)
            );
        }
        assert_eq!(
            provision_ip_certificate("192.0.2.1", || false),
            Err(CertificateProvisionError::Cancelled)
        );
    }
}
