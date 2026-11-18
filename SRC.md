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
