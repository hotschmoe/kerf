//! Kerf chat harness: a pure-logic, non-blocking Claude Messages tool loop for the teak app.
//!
//! No I/O, no threads, no clock: the host performs HTTP and runs tools; this module is the
//! state machine, request/response codecs, console view-model, tool glue and a demo script.
//!
//! Start with `Chat` (Session + ChatLog) or `Session` directly. See session.zig for the contract.
//! Modules: types (Config, HttpRequestSpec, ToolUse, ToolResult...), request, response, session,
//! chatlog, chat, tools (Engine + executeToolUse), fake_engine, mock, driver, demo, util, jsonw,
//! jsonspan.

pub const types = @import("types.zig");
pub const jsonw = @import("jsonw.zig");
pub const jsonspan = @import("jsonspan.zig");
pub const request = @import("request.zig");
pub const response = @import("response.zig");
pub const session = @import("session.zig");
pub const chatlog = @import("chatlog.zig");
pub const chat = @import("chat.zig");
pub const tools = @import("tools.zig");
pub const fake_engine = @import("fake_engine.zig");
pub const mock = @import("mock.zig");
pub const driver = @import("driver.zig");
pub const demo = @import("demo.zig");
pub const util = @import("util.zig");

pub const Session = session.Session;
pub const Step = session.Step;
pub const Event = session.Event;
pub const Chat = chat.Chat;
pub const ChatLog = chatlog.ChatLog;
pub const Config = types.Config;
pub const Engine = tools.Engine;
pub const Demo = demo.Demo;

test {
    _ = types;
    _ = jsonw;
    _ = jsonspan;
    _ = request;
    _ = response;
    _ = session;
    _ = chatlog;
    _ = chat;
    _ = tools;
    _ = fake_engine;
    _ = mock;
    _ = driver;
    _ = demo;
    _ = util;
    _ = @import("loop_test.zig");
}
