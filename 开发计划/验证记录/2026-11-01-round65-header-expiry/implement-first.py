from pathlib import Path
W=Path('/tmp/fc-round65-header-expiry/worktree')
p=W/'crates/fc-windows/src/tls_frontdoor/native/accept.rs';s=p.read_text();needle='''    pub(super) fn post_bare_accept(&mut self) -> Result<(), String> {
''';assert needle in s;s=s.replace(needle,needle+'''        self.post_bare_accept_at(Instant::now())
    }
    /// One captured origin covers preparation and the actual native post. The
    /// private explicit origin also permits precise, non-wall-clock test input.
    pub(super) fn post_bare_accept_at(&mut self, began_at: Instant) -> Result<(), String> {
        let deadline = began_at
            .checked_add(BUDGET)
            .ok_or("bare accept original deadline overflow")?;
''',1)
s=s.replace('''        let success = unsafe {
            *bare.memory.operation.get()''','''        unsafe {
            *bare.memory.operation.get()''',1)
a='''            .map_err(|e| format!("bare accept socket: {e}"))?;
            let mut received = 0;
            accept(
''';b='''            .map_err(|e| format!("bare accept socket: {e}"))?;
        }
        // A slow preparation must not issue an already-expired positive post.
        // There is no IOCP debt yet, so only the exact new socket is closed.
        if Instant::now() >= header_close_alarm(deadline) {
            self.close_bare_socket()?;
            return Err("bare accept preparation consumed original deadline".into());
        }
        // SAFETY: the same unused pinned operation/socket remains exclusively
        // owned. Native post latency consumes the already-captured interval.
        let success = unsafe {
            let bare = self.resources.accept.as_mut().unwrap();
            let mut received = 0;
            accept(
''';assert a in s;s=s.replace(a,b,1)
a='''        // Preserve the old capture position AFTER successful/IO_PENDING post.
        // This has no claim to an exact TCP establishment timestamp.
        let deadline = Instant::now() + BUDGET;
''';b='''        // A successful/PENDING call owes a real IOCP packet even if the call
        // returned too late. Publish that exact debt BEFORE negative cleanup;
        // never return an uncharged operation or mint another five seconds.
''';assert a in s;s=s.replace(a,b,1)
a='''        self.report.accept_posts += 1;
        Ok(())
''';b='''        self.report.accept_posts += 1;
        if Instant::now() >= header_close_alarm(deadline) {
            self.cancel_bare_accept(true)?;
        }
        Ok(())
''';assert a in s;s=s.replace(a,b,1);p.write_text(s)
p=W/'crates/fc-windows/src/tls_frontdoor/native.rs';s=p.read_text();a='''        for _ in 0..(CONNECTION_SLOTS + usize::from(self.http_mode)) {
            let (mut bytes''';b='''        for _ in 0..(CONNECTION_SLOTS + usize::from(self.http_mode)) {
            self.service_deadlines()?;
            let (mut bytes''';assert a in s;s=s.replace(a,b,1)
a=s.index('    fn poll_io(');b=s.index('    fn poll(&mut self)',a);part=s[a:b];assert part.endswith('        Ok(())\n    }\n');part=part[:-len('        Ok(())\n    }\n')]+'        self.service_deadlines()\n    }\n';s=s[:a]+part+s[b:]
a='''        if self.http_mode && !self.stopping {
            self.expire_bare_accept(Instant::now())?;
            for index in 0..CONNECTION_SLOTS {
                if self.resources.slots[index].phase != Phase::Closed && self.expired(index) {
                    self.close_slot(index, true)?;
                }
            }
        }
''';assert a in s;s=s.replace(a,'        self.service_deadlines()?;\n',1)
a='''        self.poll_io()?;
        for _ in 0..CONNECTION_SLOTS {
            let frame''';b='''        self.poll_io()?;
        for _ in 0..CONNECTION_SLOTS {
            self.service_deadlines()?;
            let frame''';assert a in s;s=s.replace(a,b,1)
a='''                    self.complete_frame(frame)?;
                }
                None => break,
            }
        }
        if !self.stopping {
            for index in 0..CONNECTION_SLOTS {
                self.retire_closed_slot(index)?;
            }
        }
        Ok(())
    }
    fn retire_closed_slot''';b='''                    self.complete_frame(frame)?;
                    self.service_deadlines()?;
                }
                None => break,
            }
        }
        if !self.stopping {
            for index in 0..CONNECTION_SLOTS {
                self.service_deadlines()?;
                self.retire_closed_slot(index)?;
            }
        }
        self.service_deadlines()
    }
    /// Fixed-size negative work only. This does not drain IOCP/SSPI, publish
    /// application events, allocate owners, or change any original deadline.
    fn service_deadlines(&mut self) -> Result<(), String> {
        if OWNER_FAULTED.load(Ordering::Acquire) && !self.stopping {
            return Err("TLS native cleanup fault; only original shutdown may continue".into());
        }
        if self.http_mode && !self.stopping {
            self.expire_bare_accept(Instant::now())?;
            for index in 0..CONNECTION_SLOTS {
                if self.resources.slots[index].phase != Phase::Closed && self.expired(index) {
                    self.close_slot(index, true)?;
                }
            }
        }
        Ok(())
    }
    fn retire_closed_slot''';assert a in s;s=s.replace(a,b,1);p.write_text(s)
p=W/'crates/fc-windows/src/tls_frontdoor/native/stream.rs';s=p.read_text();a='''    pub fn poll(&mut self) -> Result<Vec<TlsHttpEvent>, String> {
        self.owner.poll()?;
        Ok(self.owner.take_http_events())
    }
''';b='''    /// Service only already-due native deadlines between bounded caller work.
    /// Closing remains negative: the original IOCP/Frame debts and exact token
    /// are retained until later poll/shutdown observes their physical returns.
    pub fn service_deadlines(&mut self) -> Result<(), String> {
        self.owner.service_deadlines()
    }
    pub fn poll(&mut self) -> Result<Vec<TlsHttpEvent>, String> {
        self.owner.poll()?;
        Ok(self.owner.take_http_events())
    }
''';assert a in s;s=s.replace(a,b,1);p.write_text(s)
p=W/'crates/fc-bootstrap/src/web_transfer/network.rs';s=p.read_text();a='''        for event in events {
            self.event(event)?;
        }
        for index in 0..MAX_REQUESTS {
            let Some(mut record)''';b='''        for event in events {
            self.host.service_deadlines()?;
            self.event(event)?;
        }
        for index in 0..MAX_REQUESTS {
            self.host.service_deadlines()?;
            let Some(mut record)''';assert a in s;s=s.replace(a,b,1)
a='''                self.tick_work(&mut record)?;
                self.maybe_cleanup(&mut record)
''';b='''                self.tick_work(&mut record)?;
                self.host.service_deadlines()?;
                self.maybe_cleanup(&mut record)
''';assert a in s;s=s.replace(a,b,1)
a='''        self.retire_records()?;
        self.refresh_report();
        Ok(self.report)
''';b='''        self.retire_records()?;
        self.host.service_deadlines()?;
        self.refresh_report();
        Ok(self.report)
''';assert a in s;s=s.replace(a,b,1);p.write_text(s)
