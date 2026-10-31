//! Synchronous SSPI calls, exclusively owned by the fixed TLS worker thread.
use super::{
    credentials::Credentials, Call, CertificateThumbprint, Driver, Frame, Step, TlsError,
    BUFFER_BYTES, CONNECTION_SLOTS, PLAINTEXT_BYTES,
};
use fc_ports::http::RequestToken;
use windows::Win32::{
    Foundation::{
        SEC_E_INCOMPLETE_MESSAGE, SEC_E_OK, SEC_I_CONTEXT_EXPIRED, SEC_I_CONTINUE_NEEDED,
    },
    Security::{Authentication::Identity::*, Credentials::SecHandle},
};

const INVALID_HANDLE: SecHandle = SecHandle {
    dwLower: usize::MAX,
    dwUpper: usize::MAX,
};
const ALERT_BYTES: usize = 256;

struct Context {
    handle: SecHandle,
    token: Option<RequestToken>,
    last_token: Option<RequestToken>,
    shutting_down: bool,
    shutdown_complete: bool,
    peer_closed: bool,
    complete: bool,
    sizes: SecPkgContext_StreamSizes,
}
impl Default for Context {
    fn default() -> Self {
        Self {
            handle: INVALID_HANDLE,
            token: None,
            last_token: None,
            shutting_down: false,
            shutdown_complete: false,
            peer_closed: false,
            complete: false,
            sizes: Default::default(),
        }
    }
}
impl Context {
    fn live(&self) -> bool {
        self.handle != INVALID_HANDLE
    }
    fn bind(&mut self, token: RequestToken, call: Call) -> Result<(), TlsError> {
        if self.token == Some(token) {
            return Ok(());
        }
        let newer = self.last_token.is_none_or(|old| {
            (token.generation(), token.serial()) > (old.generation(), old.serial())
        });
        if call != Call::Handshake || self.token.is_some() || !newer {
            return Err(TlsError::InvalidInput);
        }
        self.token = Some(token);
        self.last_token = Some(token);
        Ok(())
    }
    fn retire(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
        if self.token != Some(frame.token) || frame.input_len != 0 {
            return Err(TlsError::InvalidInput);
        }
        self.reset()?;
        self.token = None;
        frame.wipe();
        Ok(Step::Retired)
    }
    fn reset(&mut self) -> Result<(), TlsError> {
        let handle = std::mem::replace(&mut self.handle, INVALID_HANDLE);
        let result = if handle != INVALID_HANDLE {
            // SAFETY: this context has no concurrent calls and never leaves its
            // creating thread. Even partially established contexts are retired.
            unsafe { DeleteSecurityContext(&handle) }.map_err(|e| TlsError::Cleanup(e.code().0))
        } else {
            Ok(())
        };
        // Keep the exact connection identity until an explicit Retire ack.
        // A failed handshake can already have deleted its partial context, but
        // that never authorizes a different token to retire/rebind this owner.
        self.shutting_down = false;
        self.shutdown_complete = false;
        self.peer_closed = false;
        self.complete = false;
        self.sizes = SecPkgContext_StreamSizes::default();
        result
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        // Explicit worker shutdown observes reset(); unwinding is best effort.
        let _ = self.reset();
    }
}

pub(super) struct SchannelSet {
    // Rust drops fields in declaration order: contexts precede credentials.
    contexts: [Context; CONNECTION_SLOTS],
    credentials: Credentials,
    cleanup_failure: Option<TlsError>,
    closed: bool,
}
impl SchannelSet {
    pub fn new(thumbprint: CertificateThumbprint) -> Result<Self, TlsError> {
        Ok(Self {
            contexts: std::array::from_fn(|_| Context::default()),
            credentials: Credentials::new(thumbprint)?,
            cleanup_failure: None,
            closed: false,
        })
    }
}
impl Driver for SchannelSet {
    fn call(&mut self, frame: &mut Frame) -> Result<Step, TlsError> {
        frame.output_len = 0;
        if self.closed {
            return Err(TlsError::Stopped);
        }
        if self.cleanup_failure.is_some() {
            return Err(TlsError::Faulted);
        }
        if frame.input_len > BUFFER_BYTES || frame.token.slot() >= CONNECTION_SLOTS {
            return Err(TlsError::InvalidInput);
        }
        let context = &mut self.contexts[frame.token.slot()];
        context.bind(frame.token, frame.call)?;
        if frame.call == Call::Retire {
            let result = context.retire(frame);
            if let Err(error @ TlsError::Cleanup(_)) = result {
                self.cleanup_failure.get_or_insert(error);
            }
            return result;
        }
        let result = match frame.call {
            Call::Handshake
                if !context.complete && !context.shutting_down && !context.peer_closed =>
            {
                handshake(context, &self.credentials, frame)
            }
            Call::Decrypt if context.complete && !context.shutting_down && !context.peer_closed => {
                decrypt(context, frame)
            }
            Call::Encrypt if context.complete && !context.shutting_down && !context.peer_closed => {
                encrypt(context, frame)
            }
            Call::Shutdown
                if context.complete && frame.input_len == 0 && !context.shutdown_complete =>
            {
                shutdown(context, &self.credentials, frame)
            }
            _ => Err(TlsError::Protocol),
        };
        if result == Ok(Step::PeerClosed) {
            // Preserve the established context for the reciprocal close_notify.
            context.peer_closed = true;
        } else if result.is_err() {
            if let Err(error) = context.reset() {
                self.cleanup_failure.get_or_insert(error);
                return Err(error);
            }
        }
        result
    }

    fn close(&mut self) -> Result<(), TlsError> {
        self.closed = true;
        for context in &mut self.contexts {
            if let Err(error) = context.reset() {
                self.cleanup_failure.get_or_insert(error);
            }
        }
        if let Err(error) = self.credentials.close() {
            self.cleanup_failure.get_or_insert(error);
        }
        self.cleanup_failure.map_or(Ok(()), Err)
    }
}

fn buffer(kind: u32, pointer: *mut u8, size: usize) -> SecBuffer {
    SecBuffer {
        cbBuffer: size as u32,
        BufferType: kind,
        pvBuffer: pointer.cast(),
    }
}
fn description(buffers: &mut [SecBuffer]) -> SecBufferDesc {
    SecBufferDesc {
        ulVersion: SECBUFFER_VERSION,
        cBuffers: buffers.len() as u32,
        pBuffers: buffers.as_mut_ptr(),
    }
}
fn checked_range(
    pointer: *mut std::ffi::c_void,
    len: usize,
    base: *const u8,
    capacity: usize,
) -> Result<std::ops::Range<usize>, TlsError> {
    let start = pointer as usize;
    let owner = base as usize;
    let offset = start.checked_sub(owner).ok_or(TlsError::Protocol)?;
    let end = offset.checked_add(len).ok_or(TlsError::Protocol)?;
    if end > capacity {
        return Err(TlsError::Protocol);
    }
    Ok(offset..end)
}
fn consumed(buffers: &[SecBuffer], input_len: usize) -> Result<usize, TlsError> {
    let mut extra = None;
    for item in buffers.iter().filter(|b| b.BufferType == SECBUFFER_EXTRA) {
        if extra.replace(item.cbBuffer as usize).is_some() {
            return Err(TlsError::Protocol);
        }
    }
    // Schannel's EXTRA describes a suffix of the original input, not a new
    // allocation. Its pvBuffer is deliberately never dereferenced.
    let used = input_len
        .checked_sub(extra.unwrap_or(0))
        .ok_or(TlsError::Protocol)?;
    if used == 0 {
        return Err(TlsError::Protocol);
    }
    Ok(used)
}

fn handshake(
    context: &mut Context,
    credentials: &Credentials,
    frame: &mut Frame,
) -> Result<Step, TlsError> {
    if frame.input_len == 0 {
        return Ok(Step::NeedInput);
    }
    let mut input = [
        buffer(SECBUFFER_TOKEN, frame.input.as_mut_ptr(), frame.input_len),
        SecBuffer::default(),
    ];
    let mut output = [
        buffer(
            SECBUFFER_TOKEN,
            frame.output.as_mut_ptr(),
            BUFFER_BYTES - ALERT_BYTES,
        ),
        buffer(
            SECBUFFER_ALERT,
            frame.output[BUFFER_BYTES - ALERT_BYTES..].as_mut_ptr(),
            ALERT_BYTES,
        ),
    ];
    let input_desc = description(&mut input);
    let mut output_desc = description(&mut output);
    let mut attributes = 0;
    let existing = context
        .live()
        .then_some(&context.handle as *const SecHandle);
    // SAFETY: all descriptors reference stable caller-owned arrays for this
    // synchronous call. No ALLOCATE_MEMORY and no client authentication requested.
    let status = unsafe {
        AcceptSecurityContext(
            Some(&credentials.handle),
            existing,
            Some(&input_desc),
            ASC_REQ_CONFIDENTIALITY
                | ASC_REQ_REPLAY_DETECT
                | ASC_REQ_SEQUENCE_DETECT
                | ASC_REQ_STREAM
                | ASC_REQ_EXTENDED_ERROR,
            0,
            Some(&mut context.handle),
            Some(&mut output_desc),
            &mut attributes,
            None,
        )
    };
    if attributes & ASC_RET_ALLOCATED_MEMORY != 0 {
        return Err(TlsError::Protocol);
    }
    if status == SEC_E_INCOMPLETE_MESSAGE {
        return Ok(Step::NeedInput);
    }
    if status != SEC_E_OK && status != SEC_I_CONTINUE_NEEDED {
        return Err(TlsError::Native(status.0));
    }
    if !context.live() || output[0].BufferType != SECBUFFER_TOKEN {
        return Err(TlsError::Protocol);
    }
    let token_len = output[0].cbBuffer as usize;
    if token_len > 0 {
        let range = checked_range(
            output[0].pvBuffer,
            token_len,
            frame.output.as_ptr(),
            BUFFER_BYTES - ALERT_BYTES,
        )?;
        frame.output.copy_within(range, 0);
    }
    frame.output_len = token_len;
    let used = consumed(&input, frame.input_len)?;
    if status == SEC_E_OK {
        // Schannel explicitly does not support ASC_REQ_INTEGRITY, which is a
        // generic SSPI sign/verify requirement, and its corresponding return
        // bit must not be assumed. TLS 1.2 record integrity remains enforced by
        // the negotiated TLS protocol and Schannel's Encrypt/DecryptMessage;
        // confidentiality, stream semantics and the negotiated protocol/cipher
        // checks below remain mandatory. No plaintext or unchecked TLS fallback.
        // https://learn.microsoft.com/en-us/windows/win32/secauthn/acceptsecuritycontext--general
        let required = ASC_RET_CONFIDENTIALITY | ASC_RET_STREAM;
        if attributes & required != required {
            return Err(TlsError::Protocol);
        }
        let mut connection = SecPkgContext_ConnectionInfo::default();
        // SAFETY: live completed context and correctly typed fixed result buffers.
        unsafe {
            QueryContextAttributesW(
                &context.handle,
                SECPKG_ATTR_CONNECTION_INFO,
                (&mut connection as *mut SecPkgContext_ConnectionInfo).cast(),
            )
            .map_err(|e| TlsError::Native(e.code().0))?;
            QueryContextAttributesW(
                &context.handle,
                SECPKG_ATTR_STREAM_SIZES,
                (&mut context.sizes as *mut SecPkgContext_StreamSizes).cast(),
            )
            .map_err(|e| TlsError::Native(e.code().0))?;
        }
        let sizes = context.sizes;
        let total = u64::from(sizes.cbHeader)
            + u64::from(sizes.cbMaximumMessage)
            + u64::from(sizes.cbTrailer);
        if connection.dwProtocol != SP_PROT_TLS1_2_SERVER
            || connection.dwCipherStrength < 128
            || sizes.cbMaximumMessage == 0
            || sizes.cbMaximumMessage as usize > PLAINTEXT_BYTES
            || sizes.cBuffers != 4
            || total > BUFFER_BYTES as u64
        {
            return Err(TlsError::Protocol);
        }
        context.complete = true;
    }
    Ok(Step::Handshake {
        consumed: used,
        complete: context.complete,
    })
}

/// Microsoft server shutdown: ApplyControlToken once, then ASC with empty
/// input until complete, send every returned token, finally explicit Retire.
/// https://learn.microsoft.com/en-us/windows/win32/secauthn/shutting-down-an-schannel-connection
fn shutdown(
    context: &mut Context,
    credentials: &Credentials,
    frame: &mut Frame,
) -> Result<Step, TlsError> {
    if !context.live() {
        return Err(TlsError::Protocol);
    }
    if !context.shutting_down {
        let mut token = SCHANNEL_SHUTDOWN;
        let mut control = [buffer(
            SECBUFFER_TOKEN,
            (&mut token as *mut u32).cast(),
            std::mem::size_of::<u32>(),
        )];
        let descriptor = description(&mut control);
        // SAFETY: synchronous call on the sole owning thread; the DWORD control
        // token remains live for this call and contains no user-provided bytes.
        unsafe { ApplyControlToken(&context.handle, &descriptor) }
            .map_err(|e| TlsError::Native(e.code().0))?;
        context.shutting_down = true;
    }
    let mut input = [
        buffer(SECBUFFER_TOKEN, std::ptr::null_mut(), 0),
        SecBuffer::default(),
    ];
    let input_desc = description(&mut input);
    let mut output = [
        buffer(
            SECBUFFER_TOKEN,
            frame.output.as_mut_ptr(),
            BUFFER_BYTES - ALERT_BYTES,
        ),
        buffer(
            SECBUFFER_ALERT,
            frame.output[BUFFER_BYTES - ALERT_BYTES..].as_mut_ptr(),
            ALERT_BYTES,
        ),
    ];
    let mut output_desc = description(&mut output);
    let mut attributes = 0;
    // SAFETY: current context, same thread, disjoint caller-owned output/alert
    // arrays. ALLOCATE_MEMORY is not requested and returned views are checked.
    let status = unsafe {
        AcceptSecurityContext(
            Some(&credentials.handle),
            Some(&context.handle),
            Some(&input_desc),
            ASC_REQ_CONFIDENTIALITY
                | ASC_REQ_REPLAY_DETECT
                | ASC_REQ_SEQUENCE_DETECT
                | ASC_REQ_STREAM
                | ASC_REQ_EXTENDED_ERROR,
            0,
            Some(&mut context.handle),
            Some(&mut output_desc),
            &mut attributes,
            None,
        )
    };
    let step = shutdown_result(frame, &output[0], status, attributes)?;
    if let Step::Shutdown { complete } = step {
        context.shutdown_complete = complete;
    }
    Ok(step)
}
fn shutdown_result(
    frame: &mut Frame,
    output: &SecBuffer,
    status: windows::core::HRESULT,
    attributes: u32,
) -> Result<Step, TlsError> {
    if attributes & ASC_RET_ALLOCATED_MEMORY != 0 || output.BufferType != SECBUFFER_TOKEN {
        return Err(TlsError::Protocol);
    }
    if status != SEC_E_OK && status != SEC_I_CONTEXT_EXPIRED && status != SEC_I_CONTINUE_NEEDED {
        return Err(TlsError::Native(status.0));
    }
    let count = output.cbBuffer as usize;
    if count > 0 {
        let range = checked_range(
            output.pvBuffer,
            count,
            frame.output.as_ptr(),
            BUFFER_BYTES - ALERT_BYTES,
        )?;
        frame.output.copy_within(range, 0);
    }
    frame.output_len = count;
    Ok(Step::Shutdown {
        complete: status == SEC_E_OK || status == SEC_I_CONTEXT_EXPIRED,
    })
}

fn decrypt(context: &mut Context, frame: &mut Frame) -> Result<Step, TlsError> {
    if frame.input_len == 0 {
        return Ok(Step::NeedInput);
    }
    let mut buffers = [
        buffer(SECBUFFER_DATA, frame.input.as_mut_ptr(), frame.input_len),
        SecBuffer::default(),
        SecBuffer::default(),
        SecBuffer::default(),
    ];
    let descriptor = description(&mut buffers);
    // SAFETY: complete context and exclusive fixed input owner; SSPI decrypts in
    // place. No buffer aliases any independent context or worker call.
    let status = unsafe { DecryptMessage(&context.handle, &descriptor, 0, None) };
    if status == SEC_E_INCOMPLETE_MESSAGE {
        return Ok(Step::NeedInput);
    }
    if status == SEC_I_CONTEXT_EXPIRED {
        return Ok(Step::PeerClosed);
    }
    // In particular, SEC_I_RENEGOTIATE is terminal: this adapter never lets a
    // post-handshake message create another authentication/authorization path.
    if status != SEC_E_OK {
        return Err(TlsError::Native(status.0));
    }
    let used = consumed(&buffers, frame.input_len);
    let result = used.and_then(|used| validate_decrypted(frame, &buffers, used));
    if result == Err(TlsError::Protocol) {
        decrypt_protocol_metadata(
            status.0,
            &buffers,
            frame.input.as_ptr(),
            frame.input_len,
            used,
        );
    }
    result
}

fn validate_decrypted(
    frame: &mut Frame,
    buffers: &[SecBuffer; 4],
    used: usize,
) -> Result<Step, TlsError> {
    let mut data = None;
    for item in buffers {
        match item.BufferType {
            SECBUFFER_DATA => {
                if data.replace(item).is_some() {
                    return Err(TlsError::Protocol);
                }
            }
            SECBUFFER_STREAM_HEADER | SECBUFFER_STREAM_TRAILER => {
                if item.cbBuffer > 0 {
                    // These native metadata views may span the original
                    // ciphertext remainder, including EXTRA. They are never
                    // copied as plaintext. Only DATA must fit consumed input;
                    // every metadata view must still fit this original owner.
                    checked_range(
                        item.pvBuffer,
                        item.cbBuffer as usize,
                        frame.input.as_ptr(),
                        frame.input_len,
                    )?;
                }
            }
            SECBUFFER_EXTRA => {} // Length-only suffix semantics, checked above.
            SECBUFFER_EMPTY if item.cbBuffer == 0 => {}
            _ => return Err(TlsError::Protocol),
        }
    }
    if let Some(data) = data {
        let len = data.cbBuffer as usize;
        if len > PLAINTEXT_BYTES {
            return Err(TlsError::Capacity);
        }
        if len > 0 {
            let range = checked_range(data.pvBuffer, len, frame.input.as_ptr(), used)?;
            frame.output[..len].copy_from_slice(&frame.input[range]);
        }
        frame.output_len = len;
    }
    Ok(Step::Plaintext { consumed: used })
}

/// Only the explicit diagnostic calls this engine. At most four Protocol
/// failures per process emit fixed numeric metadata, never decrypted bytes,
/// certificates, keys, absolute addresses or native handles.
fn decrypt_protocol_metadata(
    status: i32,
    buffers: &[SecBuffer; 4],
    base: *const u8,
    input_len: usize,
    used: Result<usize, TlsError>,
) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static EMITTED: AtomicUsize = AtomicUsize::new(0);
    if EMITTED
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            (count < 4).then_some(count + 1)
        })
        .is_err()
    {
        return;
    }
    let offset = |item: &SecBuffer| -> i64 {
        if item.pvBuffer.is_null() {
            return -1;
        }
        match (item.pvBuffer as usize).checked_sub(base as usize) {
            Some(relative) if relative <= BUFFER_BYTES => relative as i64,
            _ => -2, // Outside this input owner: never disclose the raw pointer.
        }
    };
    let used = used.map_or(-1, |bytes| bytes as i64);
    eprintln!("TLS_DECRYPT_PROTOCOL status={status} input={input_len} used={used} b0={},{},{} b1={},{},{} b2={},{},{} b3={},{},{}",
        buffers[0].BufferType,buffers[0].cbBuffer,offset(&buffers[0]),
        buffers[1].BufferType,buffers[1].cbBuffer,offset(&buffers[1]),
        buffers[2].BufferType,buffers[2].cbBuffer,offset(&buffers[2]),
        buffers[3].BufferType,buffers[3].cbBuffer,offset(&buffers[3]));
}

fn encrypt(context: &mut Context, frame: &mut Frame) -> Result<Step, TlsError> {
    let len = frame.input_len;
    let header = context.sizes.cbHeader as usize;
    let trailer = context.sizes.cbTrailer as usize;
    if len == 0 || len > PLAINTEXT_BYTES || len > context.sizes.cbMaximumMessage as usize {
        return Err(TlsError::InvalidInput);
    }
    let total = header
        .checked_add(len)
        .and_then(|n| n.checked_add(trailer))
        .filter(|n| *n <= BUFFER_BYTES)
        .ok_or(TlsError::Capacity)?;
    frame.output[header..header + len].copy_from_slice(&frame.input[..len]);
    let base = frame.output.as_mut_ptr();
    let mut buffers = [
        buffer(SECBUFFER_STREAM_HEADER, base, header),
        buffer(SECBUFFER_DATA, frame.output[header..].as_mut_ptr(), len),
        buffer(
            SECBUFFER_STREAM_TRAILER,
            frame.output[header + len..].as_mut_ptr(),
            trailer,
        ),
        SecBuffer::default(),
    ];
    let descriptor = description(&mut buffers);
    // SAFETY: fixed, disjoint in-place header/data/trailer regions; no memory
    // allocation is delegated to SSPI and all capacities were checked above.
    let status = unsafe { EncryptMessage(&context.handle, 0, &descriptor, 0) };
    if status != SEC_E_OK {
        return Err(TlsError::Native(status.0));
    }
    let expected = [
        (SECBUFFER_STREAM_HEADER, 0, header),
        (SECBUFFER_DATA, header, len),
        (SECBUFFER_STREAM_TRAILER, header + len, trailer),
    ];
    let mut compact = 0;
    for (item, (kind, start, capacity)) in buffers[..3].iter().zip(expected) {
        let count = item.cbBuffer as usize;
        if item.BufferType != kind || count > capacity || (kind == SECBUFFER_DATA && count != len) {
            return Err(TlsError::Protocol);
        }
        let range = checked_range(item.pvBuffer, count, base, total)?;
        if range.start != start {
            return Err(TlsError::Protocol);
        }
        frame.output.copy_within(range, compact);
        compact += count;
    }
    if buffers[3].BufferType != SECBUFFER_EMPTY || buffers[3].cbBuffer != 0 || compact == 0 {
        return Err(TlsError::Protocol);
    }
    frame.output_len = compact;
    Ok(Step::Encrypted)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_ranges_and_extra_never_accept_outside_or_nonprogressing_buffers() {
        let mut storage = [0u8; 32];
        let base = storage.as_mut_ptr();
        assert_eq!(checked_range(base.cast(), 32, base, 32).unwrap(), 0..32);
        assert!(checked_range(base.cast(), 33, base, 32).is_err());
        assert!(checked_range(std::ptr::null_mut(), 1, base, 32).is_err());
        assert!(checked_range(usize::MAX as *mut _, 2, base, 32).is_err());
        let extra = buffer(SECBUFFER_EXTRA, std::ptr::null_mut(), 5);
        assert_eq!(consumed(&[extra], 12), Ok(7));
        assert!(consumed(&[extra], 5).is_err());
        assert!(consumed(&[extra], 4).is_err());
        assert!(consumed(&[extra, extra], 12).is_err());
    }

    #[test]
    fn decrypted_record_keeps_extra_when_trailer_views_the_original_remainder() {
        let mut issuer = fc_ports::http::TokenIssuer::new().unwrap();
        let mut frame = Frame::new(issuer.issue(0).unwrap());
        frame.input_len = 155;
        for (index, byte) in frame.input[..155].iter_mut().enumerate() {
            *byte = index as u8;
        }
        let expected_plaintext = frame.input[13..108].to_vec();
        let expected_extra = frame.input[124..155].to_vec();
        let base = frame.input.as_mut_ptr();
        let mut buffers = [
            buffer(SECBUFFER_STREAM_HEADER, base, 13),
            buffer(SECBUFFER_DATA, frame.input[13..].as_mut_ptr(), 95),
            buffer(
                SECBUFFER_STREAM_TRAILER,
                frame.input[108..].as_mut_ptr(),
                47,
            ),
            buffer(SECBUFFER_EXTRA, frame.input[124..].as_mut_ptr(), 31),
        ];
        // Actual Win11 TLS1.2 AES-GCM descriptor: trailer is a view over tag
        // plus the unprocessed next record, rather than only the first tag.
        let used = consumed(&buffers, frame.input_len).unwrap();
        assert_eq!(used, 124);
        assert_eq!(
            validate_decrypted(&mut frame, &buffers, used),
            Ok(Step::Plaintext { consumed: 124 })
        );
        assert_eq!(frame.output_len, 95);
        assert_eq!(&frame.output[..95], expected_plaintext);
        assert_eq!(&frame.input[124..155], expected_extra);

        // Metadata cannot reach beyond the original native input owner.
        buffers[2].cbBuffer = 48;
        assert_eq!(
            validate_decrypted(&mut frame, &buffers, used),
            Err(TlsError::Protocol)
        );
        buffers[2].cbBuffer = 47;
        // DATA remains confined to consumed bytes, despite a metadata view
        // legally spanning EXTRA. No unprocessed ciphertext becomes plaintext.
        buffers[1].cbBuffer = 112;
        assert_eq!(
            validate_decrypted(&mut frame, &buffers, used),
            Err(TlsError::Protocol)
        );
        buffers[1].cbBuffer = 95;
        buffers[2] = buffers[1];
        assert_eq!(
            validate_decrypted(&mut frame, &buffers, used),
            Err(TlsError::Protocol)
        );
        buffers[2] = buffers[3];
        assert_eq!(consumed(&buffers, frame.input_len), Err(TlsError::Protocol));

        // This is the caller's subsequent compaction of the untouched suffix.
        let input_len = frame.input_len;
        frame.input.copy_within(used..input_len, 0);
        frame.input_len -= used;
        assert_eq!(frame.input_len, 31);
        assert_eq!(&frame.input[..31], expected_extra);
    }
    #[test]
    fn exact_empty_context_retirement_cannot_be_forged_and_wipes_connection_data() {
        let mut issuer = fc_ports::http::TokenIssuer::new().unwrap();
        let old = issuer.issue(0).unwrap();
        let new = issuer.issue(0).unwrap();
        let mut context = Context::default();
        assert_eq!(context.bind(old, Call::Retire), Err(TlsError::InvalidInput));
        assert_eq!(
            context.bind(old, Call::Shutdown),
            Err(TlsError::InvalidInput)
        );
        context.bind(old, Call::Handshake).unwrap();
        assert_eq!(
            context.bind(new, Call::Handshake),
            Err(TlsError::InvalidInput)
        );
        let mut forged = Frame::new(new);
        forged.call = Call::Retire;
        assert_eq!(context.retire(&mut forged), Err(TlsError::InvalidInput));
        let mut frame = Frame::new(old);
        frame.call = Call::Retire;
        frame.input.fill(17);
        frame.output.fill(29);
        frame.output_len = 12;
        assert_eq!(context.retire(&mut frame), Ok(Step::Retired));
        assert!(frame
            .input
            .iter()
            .chain(frame.output.iter())
            .all(|byte| *byte == 0));
        assert_eq!(frame.output_len, 0);
        assert_eq!(context.retire(&mut frame), Err(TlsError::InvalidInput));
        assert_eq!(
            context.bind(old, Call::Handshake),
            Err(TlsError::InvalidInput)
        );
        context.bind(new, Call::Handshake).unwrap();
    }
    #[test]
    fn error_cleanup_preserves_exact_owner_until_retire_ack() {
        let mut issuer = fc_ports::http::TokenIssuer::new().unwrap();
        let old = issuer.issue(0).unwrap();
        let new = issuer.issue(0).unwrap();
        let mut context = Context::default();
        context.bind(old, Call::Handshake).unwrap();
        // No real native handle was constructed; this models the successful
        // cleanup branch after a failed partial handshake, not Delete evidence.
        context.reset().unwrap();
        assert_eq!(context.token, Some(old));
        assert_eq!(
            context.bind(new, Call::Handshake),
            Err(TlsError::InvalidInput)
        );
        let mut frame = Frame::new(old);
        frame.call = Call::Retire;
        assert_eq!(context.retire(&mut frame), Ok(Step::Retired));
        context.bind(new, Call::Handshake).unwrap();
    }
    #[test]
    fn shutdown_returns_only_bounded_owned_token_and_explicit_completion_status() {
        let mut issuer = fc_ports::http::TokenIssuer::new().unwrap();
        let mut frame = Frame::new(issuer.issue(0).unwrap());
        frame.output[5..8].copy_from_slice(&[1, 2, 3]);
        let token = buffer(
            SECBUFFER_TOKEN,
            unsafe { frame.output.as_mut_ptr().add(5) },
            3,
        );
        assert_eq!(
            shutdown_result(&mut frame, &token, SEC_E_OK, 0),
            Ok(Step::Shutdown { complete: true })
        );
        assert_eq!(&frame.output[..3], &[1, 2, 3]);
        assert_eq!(frame.output_len, 3);
        let empty = buffer(SECBUFFER_TOKEN, std::ptr::null_mut(), 0);
        assert_eq!(
            shutdown_result(&mut frame, &empty, SEC_I_CONTEXT_EXPIRED, 0),
            Ok(Step::Shutdown { complete: true })
        );
        assert_eq!(
            shutdown_result(&mut frame, &empty, SEC_I_CONTINUE_NEEDED, 0),
            Ok(Step::Shutdown { complete: false })
        );
        let outside = buffer(SECBUFFER_TOKEN, frame.output.as_mut_ptr(), BUFFER_BYTES);
        assert_eq!(
            shutdown_result(&mut frame, &outside, SEC_E_OK, 0),
            Err(TlsError::Protocol)
        );
        assert_eq!(
            shutdown_result(&mut frame, &empty, SEC_E_OK, ASC_RET_ALLOCATED_MEMORY),
            Err(TlsError::Protocol)
        );
        let wrong = buffer(SECBUFFER_DATA, frame.output.as_mut_ptr(), 1);
        assert_eq!(
            shutdown_result(&mut frame, &wrong, SEC_E_OK, 0),
            Err(TlsError::Protocol)
        );
        assert!(matches!(
            shutdown_result(&mut frame, &empty, SEC_E_INCOMPLETE_MESSAGE, 0),
            Err(TlsError::Native(_))
        ));
    }
}
