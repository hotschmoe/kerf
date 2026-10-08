# Versioning: one source of truth (Zig repos)

This is the standard for Kerf and the owner's other Zig repos (teak, zunk, …).

## Rule
**`build.zig.zon` `.version` is the only place a version is written.** It must be valid semver. The Zig
package manager already requires this field, so there's nothing to keep in sync. Everything else derives from it:

```
build.zig.zon .version ──► build.zig (comptime @import) ──► build_options.version ──► code (`pub const version`)
                     └──► git tag "v<version>" (release CI refuses a mismatch)
```

- No version literals in source, `package.json`, docs or CI. Tests and UIs read the code constant.
- No git at configure time. Builds from a tarball or a package fetch (no `.git`) report the right version, and Zig 0.17's
  configure cache stays pure.
- On `main`, the manifest holds the version of the latest release; between releases builds report that version.
  To tell a dev build apart, pass `-Dversion-meta=<str>`, e.g. `-Dversion-meta=g$(git rev-parse --short HEAD)`. It's
  appended as semver build metadata: `0.1.0-alpha.10+g3687bbb`.
- Never embed the version in deterministic outputs (exports, goldens). Only report it in `version` commands, APIs, UI and User-Agent strings.

## build.zig (copy this)
```zig
const std = @import("std");

/// Single source of truth: build.zig.zon `.version`. `-Dversion-meta=<str>` appends "+<str>".
fn versionString(b: *std.Build) []const u8 {
    const base: []const u8 = @import("build.zig.zon").version;
    _ = std.SemanticVersion.parse(base) catch @panic("build.zig.zon .version is not valid semver");
    const meta = b.option([]const u8, "version-meta", "Semver build metadata appended as +<meta>") orelse return base;
    return b.fmt("{s}+{s}", .{ base, meta });
}

pub fn build(b: *std.Build) void {
    const options = b.addOptions();
    options.addOption([]const u8, "version", versionString(b));
    // add to every module that needs it (lib, exe, wasm, tests):
    // mod.addOptions("build_options", options);
}
```
In code: `pub const version = @import("build_options").version;`. Expose it through ONE function or constant,
which the CLI `version`/`--version`, any HTTP info endpoint, and the UI all call.

## Releasing
`tools/release.sh <semver>` bumps `.version`, commits `release: v<semver>`, tags `v<semver>` and pushes. The release
workflow starts with a step that fails unless `v` + the zon version equals the tag:
```sh
zon=$(sed -n 's/^ *\.version = "\(.*\)",$/\1/p' build.zig.zon)
test "v$zon" = "$GITHUB_REF_NAME" || { echo "::error::tag $GITHUB_REF_NAME != v$zon"; exit 1; }
```
Libraries consumed by `build.zig.zon` dependencies get the right version automatically, because the manifest
travels with the package.
