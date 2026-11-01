exec(__import__('pathlib').Path('/tmp/fc-round68-production-https/evidence/b-edit-first.py').read_text().split("edit('crates/fc-storage")[0])
# Only helpers above are loaded; the storage edits are not replayed.
for name,kind,err,count in [('uploads','Upload','UploadHubError','SLOTS'),('downloads','Download','DownloadHubError','MAX_REQUESTS')]:
 def change(s,name=name,kind=kind,err=err,count=count):
  marker=f'impl {kind}Hub {{'
  pos=s.index(marker)+len(marker)
  body=f'''
    /// Negative admission only; real file disposal remains a shared-worker job.
    pub(crate) fn request_domain_stop(&self) {{
        if let Some(shared) = self.shared.upgrade() {{ shared.cancel_all(); }}
    }}
    /// Fresh negative cleanup on the original shared workers while the rest of
    /// that pool stays live. Busy registry cells remain occupied until their
    /// original leased owner actually returns; they are never overwritten.
    pub(crate) fn cleanup_live(&self, {'app: &WebApplication, ' if name=='uploads' else ''}context: &JobContext) -> Result<{kind}HubCleanup, {err}> {{
        context.checkpoint().map_err(|_| {err}::Cancelled)?;
        let shared = self.shared.upgrade().ok_or({err}::Closed)?;
        shared.cancel_all();
        {'shared.check_application(app)?;' if name=='uploads' else ''}
        let keys: [Option<{kind}Key>; {count}] = {{
            let slots = shared.slots.try_lock().map_err(|_| {err}::Busy)?;
            std::array::from_fn(|i| match &slots[i] {{ Slot::Owned(entry) => Some(entry.key), _ => None }})
        }};
        for key in keys.into_iter().flatten() {{
            context.checkpoint().map_err(|_| {err}::Cancelled)?;
            if let Ok(mut lease) = shared.take(key) {{ {'cleanup_entry(lease.entry(), app);' if name=='uploads' else 'close_entry(lease.entry());'} }}
        }}
        {'retry_quarantined_uploads(); let _ = app.sweep_material_uploads();' if name=='uploads' else ''}
        Ok({kind}HubCleanup {{ pending_slots: shared.pending(), {'quarantined: quarantined_uploads(),' if name=='uploads' else ''} }})
    }}
'''
  s=s[:pos]+body+s[pos:]
  pos=s.index('    pub fn handle(&self)')
  s=s[:pos]+f'''    /// Trusted HTTP coordinator calls only after Net is idle and its last
    /// live-cleanup ticket has physically returned. All slots are rechecked;
    /// a Busy/running owner or quarantine cannot be retired by this flag.
    pub(crate) fn confirm_live_drained(&mut self) -> bool {{
        let shared = self.shared.as_ref().expect("live {name} owner");
        self.drained = !shared.accepting.load(Ordering::Acquire)
            && shared.pending() == 0 {'&& quarantined_uploads() == 0' if name=='uploads' else ''};
        self.drained
    }}
'''+s[pos:]
  return s
 edit(f'crates/fc-bootstrap/src/web_transfer/{name}.rs',change)
# Extract directory construction away from Backend borrow.
def voice(s):
 old='''        let mut configured = self
            .voice_files'''
 idx=s.index(old,s.index('pub fn configure_voice_files'))
 s=s[:idx]+'''        self.configure_voice_files_from_services(&backend.web_service(), backend.voice_service(), root)
    }
    pub(crate) fn configure_voice_files_from_services(
        &mut self,
        web: &crate::backend::web::WebService,
        voice: crate::backend::voice::VoiceService,
        root: &Path,
    ) -> io::Result<()> {
        if !self.service.same_instance(web) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "voice backend binding"));
        }
'''+s[idx:]
 s=s.replace('service: backend.voice_service(),\n            files:', 'service: voice,\n            files:',1)
 return s
edit('crates/fc-bootstrap/src/web_application/voice_files.rs',voice)
