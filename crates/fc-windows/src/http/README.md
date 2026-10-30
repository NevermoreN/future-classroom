# HTTP.sys fixed-slot adapter (P6-11)

`HttpHost` is infrastructure, not an authenticated business server. The caller
creates/drives/drops it on the single HTTP execution domain; it is not Send or
Sync. No thread per request, database access, file path resolution or actor
creation occurs here. Normal production application does not yet enable this
listener. The router must reserve a physical file worker and enforce identity,
Origin/CSRF, actual device/class binding and independent endpoint/body quotas.

- `ListenPrefix::https("https://school.example:443/classroom/")` requires an
  explicit scheme, host, port and trailing slash. Wildcards and plaintext LAN
  prefixes are rejected. The deployment owns URLACL, HTTPS certificate binding
  and certificate trust. Every normal request additionally needs native SSL
  evidence. Missing permission/configuration returns a native error; no fallback
  listener, firewall opening or certificate/URLACL mutation is attempted.
- `HttpHost::open(prefix)` allocates eight aligned fixed native slots. At most
  two head receives and two body streams are admitted. The native request area
  is 32 KiB including native structure/URL/pointer tables; owned wire header
  data is separately limited to 16 KiB/64 fields and target to 2048 bytes.
- `poll(0..=50)` is the sole completion domain. It returns at most sixteen owned events per call, including EOF/close control events.
  A `Request` event does not authenticate. `receive_body(token, maximum)` pulls
  one <=64 KiB piece; ordinary JSON uses <=64 KiB total, whereas an already
  admitted upload may use its independent limit (transport absolute ceiling
  50 MiB). Exactly-limit bodies still need actual EOF; one extra byte detects
  oversize without truncating acceptance. A flow holds its body-stream slot
  through EOF/cancellation. Chunked requests never become one full-body Vec.
- `respond(token, ResponseHead, chunk, more)` starts an exact Content-Length
  response. `ResponseReady` allows the next <=64 KiB piece through
  `send_response_body`; a final piece must exactly satisfy the declared total
  <=50 MiB. The memory remains pinned until the send IOCP completion. Responses
  deliberately close their connection; no hidden unbounded keepalive state.
- Request tokens come from a non-clonable `TokenIssuer`; a new issuer receives a
  fresh monotonic process generation and cannot reconstruct a running host token.
  Tokens are correlation, never authentication. Debug output redacts target,
  header names/values and body bytes.
- **The first incomplete request does not have an application-owned five-second
  header deadline.** `HeaderWait=5` is set on this URL group, but Microsoft states
  that HTTP.sys uses its driver-wide default (normally two minutes) for the first
  request on a connection: it cannot determine the URL group/request queue until
  the headers have been parsed. Server-session configuration has the same limit.
  This adapter receives no request/connection ID with which to cancel that first
  incomplete request independently. The group setting applies only to subsequent
  keep-alive requests; this adapter currently closes responses. No persistent
  driver-wide setting is changed. The Windows 11 probe observed the first partial
  header still open at its seven-second observation deadline. The product's
  required five-second initial-header guarantee is therefore **NOT_VALIDATED**,
  and P6-11 remains **NOT_RUN** pending a platform/transport decision. A posted
  empty head receive may remain pending while the server is idle. After a full
  header is delivered, application head/response handling gets five seconds and
  admitted body transfer gets an absolute 120 seconds. This application deadline
  does not substitute for the missing initial-header deadline. See
  [Microsoft timeout scope and first-request limitation](https://learn.microsoft.com/en-us/windows/win32/api/http/ns-http-http_timeout_limit_info).
- `request_stop()` withdraws the URL then shuts down the queue and submits
  request-ID cancellation using independent fixed OVERLAPPED records. Both
  immediate success and ERROR_IO_PENDING retain ownership until IOCP. The event
  low bit and skip-on-success modes are never enabled. Cancellation completion
  does not release the original operation. Continue polling to `Closed` before
  dropping. Retention never allocates replacement slots. Five-second failure to drain produces `Retained`; allocations,
  queue/session/IOCP handles remain until controlled process exit and a global
  poison gate forbids opening a replacement host. A normal fresh host is allowed
  only after the old host is completely closed.

## Explicit synthetic loopback probe

`serve_isolated(ListenPrefix::isolated_loopback(port, marker)?, seconds)` is an
explicit HTTP-only test listener on `127.0.0.1`. The marker must begin
`FutureClassroom-Test-`; timeout is 1..120 seconds. It accepts no credentials or
business routes, never changes URLACL/certificates/firewall, and does not prove
HTTPS, browser microphone support or campus reachability.

The root executable exposes only the explicit diagnostic flag
`--http-iocp-probe 54321 FutureClassroom-Test-round9 60`. It runs
`serve_isolated_twice` in one process, drains the first listener before opening
the second, checks a real first-cycle token against the second host, and prints
two numeric JSON reports. Normal application startup never invokes it.

Read-only environment checks, in PowerShell:

```powershell
netsh http show urlacl url=http://127.0.0.1:54321/FutureClassroom-Test-round9/
netsh http show servicestate view=requestq verbose=yes
whoami /groups
```

URL reservation is an OS prerequisite, not granted by SSH authorization alone.
An administrator token can register prefixes; a standard account needs an
existing matching reservation. `ERROR_ACCESS_DENIED` is reported as a missing
prerequisite. The adapter never asks for wildcard or LAN reservations. If the
root separately authorizes a temporary reservation, it must name exactly this
owned prefix/current test identity, record whether it existed first, and remove
only the reservation it created after all owned processes close. Existing user
reservations, SSL bindings and firewall rules are never cleaned up by this probe.

Routes under the marker: GET `health`, POST `echo` (only synthetic `0x5a` bytes,
<=2 MiB; reports count and byte sum), GET `download` (three fixed 64 KiB pieces),
GET `stop` (response completes before shutdown/drain). An external isolated
client can exercise fixed-length/chunked body, >16 KiB header, abrupt disconnect,
partial body timeout, streaming response and stop during an outstanding receive.
Repeat with a new host only after `Closed`; generation must increase and all
previous tokens must fail. Native metrics distinguish submissions, immediate
success, pending, inline rejection and IOCP completions. Pure ledger tests are
ownership evidence only; they are not native/TLS/Win7/Win10 runtime evidence.

## Microsoft references

- [HttpReceiveHttpRequest](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpreceivehttprequest)
- [HttpReceiveRequestEntityBody](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpreceiverequestentitybody)
- [HttpSendHttpResponse](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpsendhttpresponse)
- [HttpShutdownRequestQueue](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpshutdownrequestqueue)
- [HttpCancelHttpRequest](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpcancelhttprequest)
- [IO completion ports](https://learn.microsoft.com/en-us/windows/win32/fileio/i-o-completion-ports)
- [GetQueuedCompletionStatus](https://learn.microsoft.com/en-us/windows/win32/api/ioapiset/nf-ioapiset-getqueuedcompletionstatus)

- [Namespace reservations and registration permissions](https://learn.microsoft.com/en-us/windows/win32/http/namespace-reservations-registrations-and-routing)
- [URL prefix format and IP-bound host routing](https://learn.microsoft.com/en-us/windows/win32/http/urlprefix-strings)

The runner retains the original seven-second first-header observation and records
its EOF/reset/timeout result as `NOT_VALIDATED`, rather than relaxing it into a
pass. Remaining IOCP cancellation/drain/restart checks continue and may return
`PARTIAL` / `completionLifecycle=PASS`; that result does not mark
P6-11 complete or prove the initial-header deadline.

## Kernel connection limits remain a separate gate

Eight application slots and `HttpServerQueueLengthProperty=8` bound only this
adapter's accepted work and the associated parsed-request queue. They do not
prove an eight-connection limit on HTTP.sys connections whose first headers have
not yet been assigned to an application. No whole-kernel connection or memory
bound has been measured here.

A possible application-scoped follow-up is
`HttpSetUrlGroupProperty(HttpServerQosProperty, HTTP_QOS_SETTING_INFO)` with
`QosType=HttpQosSettingTypeConnectionLimit` and
`HTTP_CONNECTION_LIMIT_INFO { Flags.Present=1, MaxConnections=... }`. Microsoft
exposes this for outstanding connections served by a URL group; the current
adapter does **not** configure or validate this QoS capability. In particular,
that API description is not evidence that it counts the first unparsed
connections before group association. It also does not establish a five-second
first-header deadline. Evaluate actual first-connection behavior, owner scope,
resource accounting and Win7/Win10/Win11 support as part of the platform decision;
do not substitute queue/slot counts or a larger observation window for that gate.
No driver-wide connection/timeout registry or service configuration is changed.

Sources: [URL-group QoS and connection-limit setting](https://learn.microsoft.com/en-us/windows/win32/api/http/nf-http-httpseturlgroupproperty),
[connection-limit structure](https://learn.microsoft.com/en-us/windows/win32/api/http/ns-http-http_connection_limit_info),
[first-request timeout scope](https://learn.microsoft.com/en-us/windows/win32/api/http/ns-http-http_timeout_limit_info).
