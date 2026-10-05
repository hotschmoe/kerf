//! Model construction: engine, session, viewport, chat, and the boot effects.

const std = @import("std");
const alloc = @import("alloc.zig");
const model = @import("model.zig");
const session = @import("session.zig");
const engine_real = @import("engine_real.zig");
const viewport = @import("viewport.zig");
const llm = @import("../llm/mod.zig");

const gpa = alloc.gpa;

pub fn init() model.Model {
    var m: model.Model = .{};
    const eng = engine_real.engine();

    const s = gpa.create(session.Session) catch @panic("out of memory");
    s.* = session.Session.init(gpa, eng, "");
    m.doc = s;
    m.vp = viewport.Vp.init(gpa) catch @panic("viewport init failed");

    // System prompt = system.md + the engine's component catalog (SPEC 13).
    var cfg: llm.Config = .{};
    switch (eng.call(gpa, "catalog", "{\"format\":\"markdown\"}")) {
        .ok => |md| {
            defer gpa.free(md);
            cfg.system = llm.types.buildSystemText(gpa, md) catch llm.types.system_md;
        },
        .err => |e| gpa.free(e),
    }
    const chat = gpa.create(llm.Chat) catch @panic("out of memory");
    chat.* = llm.Chat.init(gpa, cfg);
    m.chat = chat;

    _ = m.fx.storageGet(.key_load, "kerf.key");
    _ = m.fx.storageGet(.settings_load, "kerf.settings");
    _ = m.fx.clock();
    // Startup parameters (?sample=truss&demo=1&tab=3d&select=sill_plate&insp=notes&prompt=...).
    _ = m.fx.queryParam(.query_sample, "sample");
    _ = m.fx.queryParam(.query_demo, "demo");
    _ = m.fx.queryParam(.query_tab, "tab");
    _ = m.fx.queryParam(.query_select, "select");
    _ = m.fx.queryParam(.query_insp, "insp");
    _ = m.fx.queryParam(.query_prompt, "prompt");
    return m;
}
