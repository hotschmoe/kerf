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

## apps/web/src/ui/panzoom.ts

The image viewer revokes object URLs it creates for SVG data; URLs passed by callers remain caller-owned.

## apps/web/src/ui/header.ts

In workspace mode, samples and opened files are imported into the library rather than replacing an unsaved scratch document.

## apps/web/src/ui/references.ts

Workspace references are loaded through `kerf serve`; their paths are specified in SPEC 23.

## apps/web/src/ui/inspector.ts

Acknowledged diagnostics appear separately from ordinary diagnostics; the list omits the corresponding `I_ACK` entries. A null compact setting selects compact mode automatically when there are many parts.

## apps/web/src/ui/console.ts

External op-log entries appear as local-agent cards. Whole-document set operations are abbreviated in the displayed tool input.

## apps/web/src/ui/status.ts

A leading `opt` marker identifies status segments hidden on narrow screens.

## apps/web/src/app.ts

Document mutations and remote reloads are serialized. Workspace undo applies only to operations on the current file and writes the restored state to disk. The client records its own writes to distinguish their `doc_changed` echoes from external edits. Part summary parsing takes the last ` x ... y ` pair because a parameter can itself contain ` x `.

## apps/web/src/engine-raw.ts

SPEC 13.1 defines the wasm ABI. Allocations can grow wasm memory, so the client constructs its memory view after allocating both inputs. The loader supplies stubs for unexpected imports even though the spec requires none.

## apps/web/src/engine.ts

The client emulates missing engine operations, recording them in `compat`; local operation application is used only when the wasm engine lacks `apply`. The HTML bootstrap may provide an already-started wasm fetch.

## apps/web/src/main.ts

Main-thread wasm is the default based on measured call sizes (see the web NOTES); `worker=1` opts in to the worker. Static builds do not probe for a workspace unless requested; serve and development builds do. UI readiness waits for the Plex Mono fonts because font-display can hide text while they load.

## apps/web/src/export.ts

Workspace exports use the file on disk. PDF is always a sheet; SVG and PNG are sheets in sheet mode; DXF is model-space only.

## apps/web/src/types.ts

SPEC 10, 11, 13 and 14 define the drawing, mesh, engine and operation types. A `BPt` contains x and y and optionally a bulge; sheet layouts and acknowledgements are specified in sections 26.2 and 25.4.

## apps/web/src/units.ts

SPEC 1 defines architectural feet-inches formatting.

## apps/web/src/draw2d.ts

SPEC 10 defines the drawing input. `View2D` maps model coordinates to screen coordinates as `(tx + zoom*x, ty - zoom*y)`. Source geometry uses flattened x/y pairs for picking, true arcs for outlines, and even-odd regions for selection tint. Picking prioritizes text boxes, nearby open paths, the smallest containing closed loop, then nearby closed outlines. Painting requires a model-to-device transform with y flipped; dragged note text can be omitted for separate offset painting.

## apps/web/src/strokefont.ts

The stroke font derives from Hershey Simplex; its data is in `spec/fonts/kerf-simplex.json`. Polylines contain flat x/y pairs, and unsupported characters are folded to ASCII look-alikes.

## apps/web/src/view2d.ts

Until the user pans or zooms, resizing refits the drawing. Note placement follows the SPEC convention: the top-left of the first text line, one cap height above its baseline. Grid levels avoid drawing coincident lines twice.

## apps/web/src/view3d.ts

Feature edges come from the mesh rather than triangle edges. The orthographic camera uses zoom for magnification and radius for its near and far planes; its front direction is +Z, azimuth turns toward +X and elevation points upward. A section cut retains z at or below the cut and uses stencil caps.

## apps/web/src/styles.css

Design tokens and rules follow `spec/DESIGN.md`.

## engine/kerf-core/src/pdf.rs

SPEC 12 defines the PDF export. It uses vector paths for text, deterministic object order and no timestamps.

## engine/kerf-core/src/pdf/writer.rs

Cross-reference entries record object-header offsets, and `startxref` points to the cross-reference table. Offsets are padded to ten digits; an offset of 10^10 or more widens its entry.

## engine/kerf-core/src/png.rs

The encoder emits deterministic grayscale PNGs. It chooses the row filter by minimum sum of signed residual magnitudes, choosing the earliest filter on ties. Adler-32 is reduced in runs of zlib's NMAX (5552) to keep its accumulator within 32 bits. The first row has a zero previous row.

## engine/kerf-core/src/png/deflate.rs

The deterministic compressor uses greedy-lazy LZ77 and hash chains with zlib level-6-style parameters (good 8, nice 128, lazy 16, chain 128), then Huffman blocks. The raw stream is wrapped by the PNG encoder. Its 64 KiB table is allocated directly on the heap to avoid a large stack temporary.

## engine/kerf-core/src/raster.rs

SPEC 12 and 28.2 govern raster export. Items use a grayscale coverage scratch with max union, composited once per item so overlapping stroke segments do not darken twice. Strokes are capsules with analytically bounded row spans; fills use eight even-odd sub-scanlines per row. Default width accounts for 90 pixels per paper inch, subject to image size limits.

## engine/kerf-core/src/raster/canvas.rs

Pixel coordinates are clamped before conversion to indices; float-to-integer conversion saturates and converts NaN to zero. Coverage is accumulated per item. The dash walker terminates because styles admit only dashes at least 0.01 mm long. Fill coverage uses min/max rather than clamp so NaN gives zero rather than propagating.

## engine/kerf-core/src/sort.rs

Geometry comparators with tolerances may be inconsistent. This fixed stable-sort algorithm never panics and always produces a permutation even then; sorted, stable output is guaranteed for strict weak orderings. It insertion-sorts blocks and merges bottom-up using a 4 KiB buffer or the Kim and Kutzner SymMerge algorithm. Larger slices sort an index permutation through one non-generic path to reduce wasm size.

## engine/kerf-core/src/num.rs

Float-to-integer conversion truncates toward zero and rejects non-finite or out-of-range values. The clamped variant maps NaN to the lower bound.

## engine/kerf-core/src/limits.rs

Input caps protect the engine from excessive work and allocation; limit errors describe how to get under the cap.

## engine/kerf-core/src/math.rs

The float routines use published musl constants and algorithms for reproducible results across targets. On wasm, compiler-builtins supplies `sqrt` as `f64.sqrt`; NaN and negative inputs use the soft result because NaN payloads are unspecified. The hardware square-root path was checked against the soft result on 302 million inputs (see engine NOTES). The huge-argument Payne-Hanek reduction uses precision 1, with two result words.

## engine/kerf-core/src/geom.rs

Bulge polylines follow DXF LWPOLYLINE semantics: a vertex's bulge is `tan(sweep/4)` for its outgoing segment, positive counterclockwise. Reversing a loop reverses segments and negates their bulges. Affine composition applies the right operand first. Orientation uses an error bound and, near zero, exact expansions with Dekker/Veltkamp product splitting. Winding-based point location treats counterclockwise loops as filled and clockwise loops as holes; the even-odd variant does not require orientation. Segment intersection can report two endpoints for collinear overlap.

## engine/kerf-core/src/pathgeom.rs

Open bulge paths have a zero bulge on their last vertex. Offsets are to the left for positive distances and miter line-line joints; fillet radii shrink where adjacent segments are too short. Ribbons have flat end caps.

## engine/kerf-core/src/pathclip.rs

Box clipping retains exact bulge arcs and lines rather than flattening them.

## engine/kerf-core/src/clip.rs

Polygon booleans split boundaries at shared crossings, merge nearby vertices within `SNAP`, classify sub-edges by midpoint and boundary direction, then chain surviving edges into loops. An endpoint within `SNAP` splits the other edge at the endpoint's own position, preventing rounding noise from separating coincident edges; proper crossings share one computed point. Filled boundaries are counterclockwise and holes clockwise. The arrangement is quadratic in the operand edge counts.

## engine/kerf-core/src/crop.rs

`W_CROP_STALE` distinguishes intentionally cropped members from members moved or resized out of a crop, and also warns when less than 10% remains visible in every drawing view. Section checks use the cut plane, elevation checks all members, and plan and iso crops are excluded because they are not model x/y crops.

## engine/kerf-core/src/annot/dims.rs

Dimensions follow SPEC 6.5, 20, 25.2 and 28.3. Non-overlapping adjacent spans on the same side can form a chain, drawn on its outermost authored line and placed and moved as a unit. Text that does not fit is placed outside instead of being reduced or omitted, preferring a chain's free end. Variants select ends, raised positions with leaders, and the side away from the object. Stacking places shorter spans first and moves conflicts outward in 0.25 paper-inch steps; pinned dimensions and chains containing them stay at their authored offset. Repair can also slide text or push a dimension, and blockers are retained for diagnostics.

## engine/kerf-core/src/annot/repair.rs

SPEC 20 and 25.2 govern bounded repair. Text first slides along a dimension line, then a dimension is pushed outward if needed; pinned dimensions and labels do not move. Labels prefer the smallest move, vertical before horizontal. Each pass reroutes notes; the loop stops when clear, after two passes without improvement, or at its pass limit, and keeps the pass with fewest hits. Moves over 1/8 paper inch are reported as `I_ANNOT_MOVED`.

## engine/kerf-core/src/annot/text.rs

SPEC 16 and 25.3 define glyph folding and case handling: significant tokens retain case. Wrapping keeps the pieces of a dimension or spacing together when they fit. Character-count wrapping splits an overlong word; paper-width wrapping leaves it whole for the caller to diagnose as overflow.

## engine/kerf-core/src/annot/callout.rs

Reference bubbles follow SPEC 26.2 and 28.8 and stay at an explicitly authored location. Without `place`, section-like views try up-right first, then the first clear candidate in eight directions at three distances; an iso uses up-right. The bubble radius fits its text and is at least 1/4 paper inch.

## engine/kerf-core/src/annot/knockout.rs

SPEC 18 requires hatch lines to be cut under notes, dimension text and labels.

## engine/kerf-core/src/annot/line.rs

Boundary lines follow SPEC 29.2 and remain at their authored positions.

## engine/kerf-core/src/annot/title.rs

View title bubbles and scale lines follow SPEC 6.4 and 6.5; layout title wrapping leaves overlong words intact.

## engine/kerf-core/src/annot/weld.rs

Weld symbols use AWS A2.4 basics, appear in section views, and stay at their explicit position. Arrow-side symbols are below the reference line and other-side symbols above it. Length and pitch use inches and sixteenths, without feet.

## engine/kerf-core/src/annot.rs

SPEC 6, 16, 20, 23, 24, 25.2, 26.2, 27.4 and 28.2 govern annotations. Layout effort can be shared across a view's passes and trials. Shared-layout keyed notes use layout-assigned numbers and leave list entries for the layout to draw. A keyed note without text first looks for the same key in its view, then in the document's views, inheriting citations if it has none; a note on nailing can derive text from its target. Leader-hit fix verification is disabled for discarded layout trials. Annotations are emitted in view order, and hatching is knocked out under text.

## engine/kerf-core/src/annot/notes.rs

SPEC 6.3, 24, 25.2, 26.2, 27.4 and 28.8 govern note routing. When a target's landing lies on an embedded bar or fastener, candidates can be taken from the target region minus those members, clear of edges by half a text height. Short drawn fasteners and end-on bar dots are obstacles to other leaders; long members and shapes that do not fill their boxes are excluded. An explicit landing is not nudged; a section landing is valid anywhere inside the crop, while an iso landing must be on a visible face or near a drawn line. Key tags use `tags_side`; `notes_side` places the list. Fix proposals reroute the entire view because clearing one leader can create another hit; verification is limited to 12 attempts and one quarter of the default routing work budget. Keyed numbers use explicit keys, then the lowest available numbers in note order; shared keys use the layout's assignments. Circle and hex tags share a size set by the widest number, with the hexagon's inscribed circle containing the number. Lists split only between entries, minimizing the taller column and preferring a longer first column on ties. Citation sections containing digits retain case because paragraph letters are significant.

## engine/kerf-core/src/annot/table.rs

SPEC 26.1 defines table schedules. Their dimensions are in paper inches regardless of view scale, and their border's top-left corner is the placement origin. Cells fill the next free slot in each row, skipping slots covered by row spans, and must fit within the columns and section. Automatic widths account for unwrapped lines and at least the widest word; overflow in fixed-width columns reports `W_TABLE_CELL`. House-style lint excludes numeric cells, while coverage reads body-row numbers.

## engine/kerf-core/src/sheet.rs

SPEC 12 and DESIGN 5 define sheet framing. Sheet geometry is built in paper inches and mapped back to model coordinates, so exporters handle it as a full-page drawing. Title-block values shrink to fit; if shrinking would go below three quarters of the normal height, a balanced two-line split is used when it yields larger text.

## engine/kerf-core/src/layout.rs

SPEC 26.2 and 28.2 define layouts and fit checks. Layout coordinates are paper inches multiplied by the first view's scale, so items from other scales, including text heights, use the ratio of scales. Explicit or document view scales are retained; iso views without their own scale use the common scale divided by `iso_ratio`. `sheet: auto` with a common scale chooses the smallest standard sheet reaching 1 inch to 1 foot, or the best achievable scale if none does. Layout note numbering merges explicit keys across views, merges unkeyed notes with matching text and citations, and numbers the resulting entries consecutively in layout order. Row titles align at the bottom, rows center across their width, and columns stack from the top.

## engine/kerf-core/src/view.rs

SPEC 6, 19, 24, 25.4, 26.1, 26.3, 28.2 and 28.5 define view parsing and selection. A schedule draws tables and notes without model geometry. Plan views look down the y axis and elevations toward negative z; plan defaults its cut to one inch below the highest drawn member. Scale zero means NTS fit. The author-set crop and scale flags distinguish explicit values from automatic fitting. Component selection combines the component's own view list with `only` and `omit` patterns, where `*` matches any run. A reference-only view with no `only` draws no model.

## engine/kerf-core/src/section.rs

SPEC 8.1 defines sections. Stroke lists retain paint order and deduplicate coincident collinear edges. Fastener positions are recorded for note landings; shared edges of monolithic castings are not drawn. A vapor retarder remains at least 0.03 paper inch from the host edge, and lumber lying in the section plane is cut lengthwise.

## engine/kerf-core/src/section/beyond.rs

In elevation, end-on lumber cut marks and loose-fill hatching remain visible only where nearer members do not hide them. Coincident slab edges of one lengthwise member are drawn once in the member's pen.

## engine/kerf-core/src/section/breaks.rs

Break lines apply to cut members and to lengthwise steel running beyond the crop.

## engine/kerf-core/src/section/cut.rs

When subtracting an embedded hole would remove more area than the hole itself, the boolean is treated as failed and the hole is skipped so the part retains its fill. Otherwise an emptied fill means the hole covers the whole cut. Hatch grain follows the longest loop edge.

## engine/kerf-core/src/section/hardware.rs

SPEC 28.4 governs hardware drawn in front: an explicit `front` wins, otherwise a section draws hardware in front when it cuts a placement member. Thin metal is widened to a minimum paper thickness; plates grow away from the touching member, or equally on both sides when neither or both edges touch. Face-on ties occlude strokes behind them.

## engine/kerf-core/src/section/joins.rs

SPEC 28.7 treats concrete slabs split from one member and concrete components sharing a `pour` as one casting. Shared edges are omitted, and joined bodies also occlude as a union so their shared boundary does not expose linework behind them.

## engine/kerf-core/src/section/nails.rs

SPEC 27.4 and 28.6 govern fasteners. A nail draws as a shank and head in plane, or as a dot end-on. Its own members and embedded members do not hide it, including when its host is cut.

## engine/kerf-core/src/section/occlusion.rs

Visible and hidden path portions are split at occluder outlines. Closed-loop pieces on either side of a visible starting point are joined.

## engine/kerf-core/src/section/strokes.rs

Coincident collinear spans keep the heavier pen, except members of one component keep a thin copy. Open strokes chain only when both pen and source match; returning to the start closes a loop.

## engine/kerf-core/src/builders/anchor_bolt.rs

SPEC 5 defines anchor bolts. A plate washer size uses `width x width x thickness`; rod extents through a host are measured along its axis from the placement point.

## engine/kerf-core/src/builders/bolt.rs

Structural bolts use a heavy hex head and nut with across-flats width `1.5d + 1/8`, head height `0.625d`, nut height `d`, an F436 washer of outside diameter `2d` and thickness `5/32`, and thread stick-out `0.5d`. The head bears at the local origin and the grip runs down or toward negative z. Weld and shear studs point up from the local origin.

## engine/kerf-core/src/builders/cmu_wall.rs

SPEC 5 defines masonry walls. Course ranges are one-based from the bottom; adjacent bond-beam courses form zones, with `bond_beam` naming the highest. Section walls use course shells, cells and bed joints; face-on walls have joint-patterned front and back shells and grouted cells.

## engine/kerf-core/src/builders/concrete.rs

SPEC 28.7 defines plan footprints as x/z outlines with y thickness. Angled edges use thin z slabs for checks, while the exact outline carried on the first slab is used by section, plan, elevation, iso and 3D drawing.

## engine/kerf-core/src/builders/connector.rs

SPEC 5 and 27.2 define connectors. Straps, ties and angles use thickened polylines or a member face; hangers, caps, bases and holdowns are built from their connected members by `hardware.rs`.

## engine/kerf-core/src/builders/fill.rs

SPEC 5 defines fill parameters and geometry.

## engine/kerf-core/src/builders/flashing.rs

SPEC 5 defines flashing parameters and geometry.

## engine/kerf-core/src/builders/hardware.rs

SPEC 27.2 defines framing hardware. Plates are placed in world coordinates and made local to the placement point so the section, plan, elevation, iso and mesh agree. A truss hanger seats its bottom chord profile. Post-cap coverage depends on the bearing overlap, with a separate case for a beam butting at a post edge. `W_HOLDOWN_ROD` suggestions use a rod within eight inches in plan and z and drawn in a view with the holdown.
