//! The embedded web UI (spec/SERVE.md): static file serving from the ui_assets module, the page shown when the binary has no UI, and the Content-Security-Policy with the hashes of the inline bootstrap scripts.

const std = @import("std");
const kerf = @import("kerf");
const http = @import("../http.zig");
const ui_assets = @import("ui_assets");
const Io = std.Io;
const Allocator = std.mem.Allocator;
const serve = @import("../serve.zig");
const Server = serve.Server;

pub fn static(s: *Server, a: Allocator, req: http.Request, w: *Io.Writer, path: []const u8) !bool {
    const ka = req.keep_alive;
    const head_only = std.mem.eql(u8, req.method, "HEAD");
    var rel = std.mem.trimStart(u8, path, "/");
    if (rel.len == 0) rel = "index.html";
    if (std.mem.indexOf(u8, rel, "..") != null or std.mem.indexOfScalar(u8, rel, '\\') != null) {
        try http.sendError(a, w, 400, ka, "", "E_PATH", "bad path");
        return ka;
    }
    for (ui_assets.files) |f| {
        if (!std.mem.eql(u8, f.path, rel)) continue;
        const h: http.Head = .{ .status = 200, .content_type = http.mimeFor(f.path), .keep_alive = ka, .no_store = false, .extra = "Cache-Control: no-cache\r\n", .csp = s.ui_csp };
        if (head_only) try http.sendHeadOnly(w, h, f.data.len) else try http.send(w, h, f.data);
        return ka;
    }
    if (ui_assets.files.len == 0 and std.mem.eql(u8, rel, "index.html")) {
        const h: http.Head = .{ .status = 200, .content_type = "text/html; charset=utf-8", .keep_alive = ka };
        if (head_only) try http.sendHeadOnly(w, h, no_ui_page.len) else try http.send(w, h, no_ui_page);
        return ka;
    }
    try http.sendError(a, w, 404, ka, "", "E_NOT_FOUND", "not found");
    return ka;
}

/// The UI is same-origin only: scripts from this server (plus the hashes of the inline bootstrap scripts of the
/// embedded index.html, computed at startup), wasm compilation, no framing, no plugins, no form posts.
pub const ui_csp_prefix = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'";

pub const ui_csp_suffix = "; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self' data: blob:; worker-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'";

pub const default_ui_csp = ui_csp_prefix ++ ui_csp_suffix;

/// `default_ui_csp` plus `'sha256-...'` for every inline `<script>` of the embedded index.html.
pub fn buildUiCsp(gpa: Allocator) ![]const u8 {
    var out: std.ArrayList(u8) = .empty;
    errdefer out.deinit(gpa);
    try out.appendSlice(gpa, ui_csp_prefix);
    for (ui_assets.files) |f| {
        if (!std.mem.eql(u8, f.path, "index.html")) continue;
        var rest: []const u8 = f.data;
        while (std.mem.indexOf(u8, rest, "<script")) |i| {
            rest = rest[i + "<script".len ..];
            const tag_end = std.mem.indexOfScalar(u8, rest, '>') orelse break;
            const attrs = rest[0..tag_end];
            const close = std.mem.indexOf(u8, rest[tag_end + 1 ..], "</script>") orelse break;
            const body = rest[tag_end + 1 ..][0..close];
            rest = rest[tag_end + 1 + close ..];
            if (std.mem.indexOf(u8, attrs, "src=") != null or body.len == 0) continue;
            var digest: [32]u8 = undefined;
            std.crypto.hash.sha2.Sha256.hash(body, &digest, .{});
            var b64: [44]u8 = undefined;
            _ = std.base64.standard.Encoder.encode(&b64, &digest);
            try out.print(gpa, " 'sha256-{s}'", .{&b64});
        }
    }
    try out.appendSlice(gpa, ui_csp_suffix);
    return out.toOwnedSlice(gpa);
}

pub const no_ui_page =
    \\<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
    \\<title>kerf serve</title>
    \\<style>body{font:15px/1.5 ui-monospace,Menlo,Consolas,monospace;max-width:46rem;margin:3rem auto;padding:0 1rem;background:#f4efe3;color:#1d2a3a}
    \\code,pre{background:#e9e2d0;padding:.1rem .3rem}pre{padding:.7rem;overflow:auto}h1{font-size:1.2rem;border-bottom:2px solid #1d2a3a}</style></head><body>
    \\<h1>KERF SERVE</h1>
    \\<p>The server is running, but this binary was built <b>without the web UI</b>.</p>
    \\<p>Build the UI, then rebuild the CLI with it embedded:</p>
    \\<pre>cd apps/web &amp;&amp; npm ci &amp;&amp; npm run build:serve
    \\cd ../../engines/zig &amp;&amp; zig build -Doptimize=ReleaseSmall -Dui=../../apps/web/dist-serve</pre>
    \\<p>Release binaries from GitHub already include the UI. The API works regardless:
    \\<a href="/api/info">/api/info</a> &middot; <a href="/api/docs">/api/docs</a></p>
    \\</body></html>
;
