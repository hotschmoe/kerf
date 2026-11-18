# Source notes

## engine/kerf-wasm/src/lib.rs

The host must call exports serially. `kerf_out_ptr` and `kerf_out_len` describe output valid until the next call. The ABI is specified in `spec/SPEC.md` section 13.1.

## engine/kerf-wasm/src/heap.rs

The single-threaded allocator grows wasm linear memory and reuses freed blocks through size-class free lists. Blocks are powers of two with a minimum size of 16 bytes.

## engine/kerf-cli/src/sys/mod.rs

The platform wrappers provide process groups, shutdown signals, random bytes, file locks and executable lookup.

## engine/kerf-cli/src/sys/unix.rs

Agents lead separate process groups; shutdown signals address the group. File locking tolerates filesystems that report locking as unsupported.

## engine/kerf-cli/src/sys/windows.rs

Agents start suspended in a separate process group and join a kill-on-close job before resuming. If job setup fails, termination is limited to the leader and descendants may survive. Console close, logoff and shutdown handlers wait ten seconds so the server can stop agents before the process ends. The log lock is placed beyond its contents because Windows byte-range locks are mandatory; if locking fails, the server mutex still serializes its own writes. Executable lookup follows `PATH` and runnable `PATHEXT` entries (`exe`, `com`, `cmd`, `bat`), including npm `.cmd` shims. Verbatim path prefixes are removed for display.

## engine/kerf-cli/src/serve/agent.rs

Attachment directories must be real directories rather than symlinks. Base64 input requires standard padding, no whitespace and zero trailing bits.

## engine/kerf-cli/src/serve/conn.rs

Each connection runs on its own thread. Each phase arms a deadline; a watchdog shuts down the socket on expiry to wake blocked I/O. A zero deadline disarms it while an SSE stream waits.

## engine/kerf-cli/src/serve/docs.rs

A reference download must be listed by its document, remain inside the served folder after resolving symlinks, and be an image or PDF.

## engine/kerf-cli/src/serve/ui.rs

The browser receives the default style merged with a workspace style under SPEC 25.6. The UI content-security policy includes a SHA-256 source for each inline script in the embedded index.

## engine/kerf-cli/src/events.rs

The bounded SSE ring never blocks publishers for a slow client; a client can lose frames that have aged out of the ring.

## engine/kerf-cli/src/hash.rs

Document ETags use the final4 Wyhash variant for their content hash.

## engine/kerf-cli/src/http.rs

HTTP request parsing rejects malformed request heads and bounds both fixed-length and chunked bodies. Secret comparisons scan to the longer input length without content-dependent early exit.

## engine/kerf-cli/src/workspace.rs

The op log is adjacent to its document and is the only location for nondeterministic timestamps. A document's lock covers the full read-modify-write and log append; unsupported filesystem locks are tolerated. Atomic writes sync the new file before rename and attempt to sync the parent directory afterward. Log paths refuse symlinks, and reference paths must first pass path validation and are then canonicalized and rejected if they escape the served root. Workspace style lookup uses the nearest style in the document folder or its ancestors, as specified in SPEC 25.6.

## engine/kerf-cli/src/agents.rs

Workspace agent templates cannot replace built-ins or execute detection commands unless trusted; the workspace configuration file itself cannot be a symlink. The `{resume}` argument expands only when a session ID is present; session IDs are collected from `session_id`, `sessionId`, `thread_id`, or a pi session event. Agent pipe output is drained before the server publishes document changes and emits the exit event. Detection is cached for 30 seconds, with concurrent callers sharing a probe. On Unix, shutdown sends the process group a soft stop followed by a hard stop after the grace period; Windows uses a hard termination for both.

## engine/kerf-cli/src/serve.rs

The workspace directory is the source of truth. A poller notices external document edits and publishes SSE events; server-originated writes update the scan state before publishing so they are not echoed as external edits. Only regular document files are served. Log scanning skips a line larger than its read window so it does not retry that line forever.

## engine/kerf-cli/src/main.rs

The panic hook reports internal failures and exits with EX_SOFTWARE rather than allowing the abort profile to produce a core dump. Document writers hold the workspace lock across read, edit, write and log append; `--if-match` rejects stale writes. A log append failure occurs after the document has been written and is reported with exit code 3. The CLI folds non-ASCII text for Windows consoles. Workspace style precedence and accepted ops forms are specified in SPEC 25.6 and 21.

## apps/web/src/chat/agent.ts

The agent runner consumes the local server's SSE stream. Event shapes for the supported agents are illustrated by the recorded fixtures under `apps/web/test/fixtures/agent-*.jsonl`. If the event stream remains offline for 20 seconds, the run can no longer report an exit and the UI stops waiting. Unrecognized events are kept in `window.__kerf.agent.unknown` for debugging.

## apps/web/src/chat/session.ts

Chat is available only when the UI has a workspace served by `kerf serve`; it runs local agent CLIs.

## apps/web/src/chat/store.ts

Agent choice and resume sessions are stored per agent and document. Local storage can be unavailable, so access failures are tolerated; selection falls back to the first available agent and then the first listed agent.

## apps/web/src/workspace/client.ts

A token supplied in the URL is stored in session storage and removed from the address bar. HTTP requests use a bearer header; the SSE connection uses a query token because EventSource cannot set that header. Both named SSE events and unnamed JSON messages are accepted.

## apps/web/src/workspace/workspace.ts

Documents are compared with key-sorted JSON so key order alone does not trigger reloads. SSE agent events can precede the run's POST response, so they are buffered until the handler is registered. A UI-started run suppresses duplicate log cards during the run and briefly afterward because the server's folder poller can publish its log entries after the exit event.
