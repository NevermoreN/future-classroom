//! Endpoint notifications live only for the audio execution domain. Callbacks
//! perform bounded opaque-ID comparisons and a nonblocking atomic stop signal.
use std::{
    marker::PhantomData,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};
use windows::{
    core::{Error, IUnknown, IUnknown_Vtbl, Interface, Result, GUID, HRESULT, PCWSTR},
    Win32::{
        Foundation::{E_FAIL, E_NOINTERFACE, E_POINTER, PROPERTYKEY},
        Media::Audio::*,
        System::Com::*,
    },
};
static ACTIVE: AtomicBool = AtomicBool::new(false);
static POISONED: AtomicBool = AtomicBool::new(false);
const MAX_ID_UNITS: usize = 32_768;

struct Apartment;
impl Apartment {
    fn initialize() -> Result<Self> {
        // SAFETY: constructed solely in the audio owner thread, never moved to
        // another thread. Every successful initialization has one matching Drop.
        unsafe {
            CoInitializeEx(None, COINIT_MULTITHREADED).ok()?;
        }
        Ok(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: monitor's COM interfaces are declared before this field and
        // therefore released before the originating thread uninitializes COM.
        unsafe {
            CoUninitialize();
        }
    }
}
struct Reservation;
impl Drop for Reservation {
    fn drop(&mut self) {
        ACTIVE.store(false, Ordering::Release);
    }
}

struct Signals {
    active: AtomicBool,
    callbacks: AtomicU32,
    stop: Arc<dyn Fn() + Send + Sync>,
}
struct Enter<'a>(&'a Signals);
impl Drop for Enter<'_> {
    fn drop(&mut self) {
        self.0.callbacks.fetch_sub(1, Ordering::AcqRel);
    }
}
impl Signals {
    fn enter(&self) -> Enter<'_> {
        self.callbacks.fetch_add(1, Ordering::AcqRel);
        Enter(self)
    }
    fn stop(&self) {
        if self.active.load(Ordering::Acquire) {
            // Composition promises this callback only invalidates atomic
            // capabilities/signals; it must not wait, lock, or access DB/UI.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (self.stop)()));
        }
    }
}
fn same_id(pointer: &PCWSTR, expected: &Option<Vec<u16>>) -> bool {
    let Some(expected) = expected else {
        return pointer.is_null();
    };
    if pointer.is_null() {
        return false;
    }
    for (index, unit) in expected
        .iter()
        .copied()
        .chain(std::iter::once(0))
        .enumerate()
    {
        // SAFETY: API ID strings remain valid/NUL-terminated for this call. Stop
        // at an earlier NUL and read no more than the bounded known ID + NUL.
        let actual = unsafe { pointer.0.add(index).read() };
        if actual != unit {
            return false;
        }
        if actual == 0 {
            return true;
        }
    }
    false
}
// Microsoft Device Events uses only IUnknown + IMMNotificationClient. The
// windows-rs implement macro also exposes a WeakRefCount tear-off whose weak
// IMarshal imports combase.dll even with Agile=false. This small typed boundary
// deliberately implements those two interfaces only, avoiding the post-baseline
// combase.dll import; actual Windows 7 execution remains unvalidated.
// The MTA owner never exports this object to another apartment; callbacks touch
// only immutable IDs and atomic/thread-safe state. No unsafe Send/Sync is used.
#[repr(C)]
struct Client {
    vtable: &'static IMMNotificationClient_Vtbl,
    references: AtomicU32,
    signals: Arc<Signals>,
    console: Option<Vec<u16>>,
    multimedia: Option<Vec<u16>>,
}
impl Client {
    fn interface(
        signals: Arc<Signals>,
        console: Option<Vec<u16>>,
        multimedia: Option<Vec<u16>>,
    ) -> IMMNotificationClient {
        let owned = Box::new(Self {
            vtable: &CLIENT_VTABLE,
            references: AtomicU32::new(1),
            signals,
            console,
            multimedia,
        });
        // SAFETY: repr(C)'s first field is the exact static IMMNotificationClient
        // vtable pointer. This transfers the sole initial COM reference; Release
        // reconstructs the allocation only on the final atomic reference.
        unsafe { IMMNotificationClient::from_raw(Box::into_raw(owned).cast()) }
    }
}
const CLIENT_VTABLE: IMMNotificationClient_Vtbl = IMMNotificationClient_Vtbl {
    base__: IUnknown_Vtbl {
        QueryInterface: client_query,
        AddRef: client_add_ref,
        Release: client_release,
    },
    OnDeviceStateChanged: client_state,
    OnDeviceAdded: client_added,
    OnDeviceRemoved: client_removed,
    OnDefaultDeviceChanged: client_default,
    OnPropertyValueChanged: client_property,
};
unsafe extern "system" fn client_query(
    this: *mut std::ffi::c_void,
    iid: *const GUID,
    output: *mut *mut std::ffi::c_void,
) -> HRESULT {
    if iid.is_null() || output.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM supplies valid iid/output storage and a live interface reference.
    unsafe {
        *output = std::ptr::null_mut();
        if *iid == IUnknown::IID || *iid == IMMNotificationClient::IID {
            client_add_ref(this);
            *output = this;
            HRESULT(0)
        } else {
            E_NOINTERFACE
        }
    }
}
unsafe extern "system" fn client_add_ref(this: *mut std::ffi::c_void) -> u32 {
    // SAFETY: a live COM reference or QueryInterface retains this fixed allocation.
    let client = unsafe { &*this.cast::<Client>() };
    match client
        .references
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |count| {
            count.checked_add(1)
        }) {
        Ok(previous) => previous + 1,
        // Never wrap the count and permit premature free; no unwinding across ABI.
        Err(_) => std::process::abort(),
    }
}
unsafe extern "system" fn client_release(this: *mut std::ffi::c_void) -> u32 {
    // SAFETY: caller releases one owned COM reference. Monitor keeps its reference
    // until unregister and callback drain; hence no callback aliases final free.
    let client = unsafe { &*this.cast::<Client>() };
    let previous = client.references.fetch_sub(1, Ordering::Release);
    if previous == 0 {
        std::process::abort();
    }
    if previous == 1 {
        std::sync::atomic::fence(Ordering::Acquire);
        // SAFETY: this was constructed by Box::into_raw; final reference is gone,
        // callbacks have drained, and no Rust borrow survives this final access.
        let destroyed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: ownership is transferred back once after the last atomic
            // release. Pointer is not observed again after this allocation drops.
            unsafe {
                drop(Box::from_raw(this.cast::<Client>()));
            }
        }));
        if destroyed.is_err() {
            std::process::abort();
        }
    }
    previous - 1
}
macro_rules! notification {
    ($name:ident, $method:ident, $( $parameter:ident : $ty:ty => $argument:expr ),*) => {
        unsafe extern "system" fn $name(this: *mut std::ffi::c_void, $( $parameter:$ty ),*) -> HRESULT {
            // SAFETY: MMDevice invokes the typed vtable on a registered live
            // object; retained registration + in-flight drain protect this borrow.
            let client = unsafe { &*this.cast::<Client>() };
            // Keep Rust panics out of the COM ABI, even during diagnostic hooks.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| client.$method($( $argument ),*)))
                .map_or(E_FAIL, Into::into)
        }
    };
}
notification!(client_state, OnDeviceStateChanged, id:PCWSTR => &id, state:DEVICE_STATE => state);
notification!(client_added, OnDeviceAdded, id:PCWSTR => &id);
notification!(client_removed, OnDeviceRemoved, id:PCWSTR => &id);
notification!(client_default, OnDefaultDeviceChanged, flow:EDataFlow => flow, role:ERole => role, id:PCWSTR => &id);
notification!(client_property, OnPropertyValueChanged, id:PCWSTR => &id, key:PROPERTYKEY => &key);
#[allow(non_snake_case)]
impl Client {
    fn OnDefaultDeviceChanged(&self, flow: EDataFlow, role: ERole, id: &PCWSTR) -> Result<()> {
        let _entered = self.signals.enter();
        if flow == eRender
            && ((role == eConsole && !same_id(id, &self.console))
                || (role == eMultimedia && !same_id(id, &self.multimedia)))
        {
            self.signals.stop();
        }
        Ok(())
    }
    fn OnDeviceStateChanged(&self, id: &PCWSTR, state: DEVICE_STATE) -> Result<()> {
        let _entered = self.signals.enter();
        if state != DEVICE_STATE_ACTIVE
            && (same_id(id, &self.console) || same_id(id, &self.multimedia))
        {
            self.signals.stop();
        }
        Ok(())
    }
    fn OnDeviceRemoved(&self, id: &PCWSTR) -> Result<()> {
        let _entered = self.signals.enter();
        if same_id(id, &self.console) || same_id(id, &self.multimedia) {
            self.signals.stop();
        }
        Ok(())
    }
    fn OnDeviceAdded(&self, _: &PCWSTR) -> Result<()> {
        let _entered = self.signals.enter();
        Ok(())
    }
    fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> Result<()> {
        let _entered = self.signals.enter();
        Ok(())
    }
}
fn endpoint_id(enumerator: &IMMDeviceEnumerator, role: ERole) -> Result<Option<Vec<u16>>> {
    // SAFETY: COM initialized on this thread; interface remains owned in Registration.
    let endpoint = match unsafe { enumerator.GetDefaultAudioEndpoint(eRender, role) } {
        Ok(endpoint) => endpoint,
        Err(error) if error.code().0 as u32 == 0x80070490 => return Ok(None), // E_NOTFOUND
        Err(error) => return Err(error),
    };
    // SAFETY: returned PWSTR is an allocated opaque endpoint ID; always free it.
    let id = unsafe { endpoint.GetId()? };
    let mut units = Vec::new();
    let mut result = Err(Error::from_hresult(E_FAIL));
    if !id.is_null() {
        for index in 0..MAX_ID_UNITS {
            // SAFETY: GetId returns NUL-terminated COM allocated storage.
            let unit = unsafe { id.0.add(index).read() };
            if unit == 0 {
                result = Ok(Some(units));
                break;
            }
            units.push(unit);
        }
    }
    // SAFETY: GetId transfers CoTaskMem ownership, including an empty/null result.
    unsafe {
        CoTaskMemFree(Some(id.0.cast()));
    }
    result
}
struct Registration {
    client: IMMNotificationClient,
    enumerator: IMMDeviceEnumerator,
    signals: Arc<Signals>,
    registered: bool,
    _apartment: Apartment,
    _reservation: Reservation,
}
/// !Send/!Sync: registration, unregister and COM release remain in the audio
/// owner thread. `stop` must be a pure atomic invalidation/control notification.
pub struct DefaultRenderMonitor {
    registration: Option<Registration>,
    _thread: PhantomData<Rc<()>>,
}
impl DefaultRenderMonitor {
    pub fn start(stop: Arc<dyn Fn() + Send + Sync>) -> Result<Self> {
        if POISONED.load(Ordering::Acquire)
            || ACTIVE
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
        {
            return Err(Error::from_hresult(E_FAIL));
        }
        let reservation = Reservation;
        let apartment = Apartment::initialize()?;
        // SAFETY: MTA is initialized and CLSID requests the platform enumerator.
        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_INPROC_SERVER)? };
        let signals = Arc::new(Signals {
            active: AtomicBool::new(true),
            callbacks: AtomicU32::new(0),
            stop,
        });
        let console = endpoint_id(&enumerator, eConsole)?;
        let multimedia = endpoint_id(&enumerator, eMultimedia)?;
        let client = Client::interface(signals.clone(), console.clone(), multimedia.clone());
        // SAFETY: owned client reference is retained through successful unregister
        // and all in-flight callbacks. Register itself does not AddRef the client.
        unsafe {
            enumerator.RegisterEndpointNotificationCallback(&client)?;
        }
        let monitor = Self {
            registration: Some(Registration {
                client,
                enumerator,
                signals: signals.clone(),
                registered: true,
                _apartment: apartment,
                _reservation: reservation,
            }),
            _thread: PhantomData,
        };
        // Close the snapshot/register race: a missed pre-registration change is
        // caught by this second snapshot, while post-registration changes notify.
        let registered = monitor.registration.as_ref().expect("just initialized");
        if endpoint_id(&registered.enumerator, eConsole)? != console
            || endpoint_id(&registered.enumerator, eMultimedia)? != multimedia
        {
            signals.stop();
        }
        Ok(monitor)
    }
    /// Disable callbacks first; unregister outside callbacks; release the client,
    /// enumerator and apartment only after unregister + in-flight drain.
    pub fn close(&mut self) -> Result<bool> {
        let Some(owned) = self.registration.as_mut() else {
            return Ok(true);
        };
        owned.signals.active.store(false, Ordering::Release);
        if owned.registered {
            // SAFETY: same exact client registered above, both references remain
            // owned here and this is never called from IMMNotificationClient.
            unsafe {
                owned
                    .enumerator
                    .UnregisterEndpointNotificationCallback(&owned.client)?;
            }
            owned.registered = false;
        }
        if owned.signals.callbacks.load(Ordering::Acquire) != 0 {
            return Ok(false);
        }
        self.registration = None;
        Ok(true)
    }
}
impl Drop for DefaultRenderMonitor {
    fn drop(&mut self) {
        if self.close() != Ok(true) {
            POISONED.store(true, Ordering::Release);
            // Unregister failure/late callback cannot justify deleting its `this`
            // pointer. Bound retention to this sole monitor, until process exit.
            if let Some(owned) = self.registration.take() {
                std::mem::forget(owned);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::{Interface, GUID};
    #[test]
    fn endpoint_changes_ignore_capture_other_roles_and_unrelated_usb() {
        let calls = Arc::new(AtomicU32::new(0));
        let observed = calls.clone();
        let signals = Arc::new(Signals {
            active: AtomicBool::new(true),
            callbacks: AtomicU32::new(0),
            stop: Arc::new(move || {
                observed.fetch_add(1, Ordering::AcqRel);
            }),
        });
        let client = Client::interface(signals.clone(), Some(vec![65]), Some(vec![65]));
        fn send_sync<T: Send + Sync>() {}
        send_sync::<Client>();
        for iid in [
            GUID::from_u128(0x94ea2b94_e9cc_49e0_c0ff_ee64ca8f5b90), // IAgileObject
            GUID::from_u128(0x00000003_0000_0000_c000_000000000046), // IMarshal
        ] {
            let mut pointer = std::ptr::null_mut();
            // SAFETY: iid/output are live for this synchronous QI. Unsupported
            // interfaces must return E_NOINTERFACE without a retained reference.
            let result = unsafe { client.query(&iid, &mut pointer) };
            assert_eq!(result, windows::Win32::Foundation::E_NOINTERFACE);
            assert!(pointer.is_null());
        }
        let a = [65, 0];
        let b = [66, 0];
        // SAFETY: test pointers reference live, bounded NUL-terminated UTF-16.
        unsafe {
            client
                .OnDefaultDeviceChanged(eCapture, eConsole, PCWSTR(b.as_ptr()))
                .unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eCommunications, PCWSTR(b.as_ptr()))
                .unwrap();
            client.OnDeviceAdded(PCWSTR(b.as_ptr())).unwrap();
            client.OnDeviceRemoved(PCWSTR(b.as_ptr())).unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eConsole, PCWSTR(a.as_ptr()))
                .unwrap();
            assert_eq!(calls.load(Ordering::Acquire), 0);
            client
                .OnDeviceStateChanged(PCWSTR(a.as_ptr()), DEVICE_STATE_UNPLUGGED)
                .unwrap();
            client
                .OnDefaultDeviceChanged(eRender, eMultimedia, PCWSTR(b.as_ptr()))
                .unwrap();
            assert_eq!(calls.load(Ordering::Acquire), 2);
            signals.active.store(false, Ordering::Release);
            client.OnDeviceRemoved(PCWSTR(a.as_ptr())).unwrap();
        }
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert_eq!(signals.callbacks.load(Ordering::Acquire), 0);
        let identity = client.cast::<IUnknown>().unwrap();
        let retained = identity.cast::<IMMNotificationClient>().unwrap();
        assert_eq!(Interface::as_raw(&retained), Interface::as_raw(&client));
        drop(identity);
        drop(client);
        assert_eq!(Arc::strong_count(&signals), 2);
        drop(retained);
        assert_eq!(Arc::strong_count(&signals), 1);
    }
}
