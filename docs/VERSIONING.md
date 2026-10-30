# Versioning: one source of truth

**`version` under `[workspace.package]` in `engine/Cargo.toml` is the only place a version is written.** It must be
valid semver. Everything else derives from it:

```
engine/Cargo.toml [workspace.package].version ──► every crate (version.workspace = true)
                                              ├──► kerf_core::api::VERSION (env!("CARGO_PKG_VERSION"))
                                              │      ──► `kerf version`, `kerf --version`, GET /api/info, the wasm `version` call
                                              └──► git tag "v<version>" (release CI refuses a mismatch)
```

- No version literals in source, `package.json`, docs or CI. Tests and UIs read the code constant.
- Never embed the version in deterministic outputs (exports, goldens): the PDF `Producer` is just `kerf`. Report the
  version only in `version` commands, APIs, UI and User-Agent strings.
- `engine/Cargo.lock` is committed and carries the same version for the three workspace packages; `cargo build` keeps it
  in step with the manifest.

## Releasing
`tools/release.sh <semver>` bumps the workspace version, commits `release: v<semver>`, tags `v<semver>` and pushes. The
release workflow starts with a step that fails unless `v` + the manifest version equals the tag:
```sh
ver=$(sed -n '/^\[workspace.package\]/,/^\[/s/^version = "\(.*\)"$/\1/p' engine/Cargo.toml)
test "v$ver" = "$GITHUB_REF_NAME" || { echo "::error::tag $GITHUB_REF_NAME != v$ver"; exit 1; }
```
On `main` the manifest holds the version of the latest release; between releases builds report that version.
