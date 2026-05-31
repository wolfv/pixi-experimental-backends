# pixi-experimental-backends

Experimental [pixi](https://pixi.sh) build backends for languages and build
systems that aren't covered by the in-tree backends yet.

| Backend | Builds |
| --- | --- |
| `pixi-build-meson` | Projects using [Meson](https://mesonbuild.com/) |
| `pixi-build-autotools` | Projects using GNU Autotools (`configure` / `autoreconf`) |
| `pixi-build-make` | Projects using plain GNU Make |
| `pixi-build-go` | Go modules (cgo-aware, produces conda packages) |
| `pixi-build-gradle` | JVM projects built with Gradle (incl. `gradlew`) |
| `pixi-build-nodejs` | Node.js apps via npm / yarn / pnpm / bun |
| `pixi-build-bazel` | Projects built with [Bazel](https://bazel.build/) |

All backends speak the pixi build API (`pixi-build-api-version >=4,<5`) and
are distributed as conda packages on the
[`pixi-experimental-backends`](https://prefix.dev/channels/pixi-experimental-backends)
channel on prefix.dev.

## Using a backend

Add the channel and reference the backend from a package's
`[package.build.backend]` section. Minimal example using Meson:

```toml
[workspace]
channels = [
  "https://prefix.dev/pixi-experimental-backends",
  "https://prefix.dev/conda-forge",
]
platforms = ["linux-64", "linux-aarch64", "osx-arm64", "win-64"]
preview = ["pixi-build"]

[package]
name = "libopus"
version = "1.5.2"
license = "BSD-3-Clause"

[package.build.source]
url = "https://downloads.xiph.org/releases/opus/opus-1.5.2.tar.gz"

[package.build.backend]
name = "pixi-build-meson"
version = ">=0.1.0"

[package.build.config]
compilers = ["c"]
extra-args = ["-Dtests=disabled", "-Ddocs=disabled"]
```

Then run `pixi build` in that directory to produce a `.conda` package.

## Examples

The [`examples/`](./examples) directory has a working `pixi.toml` for each
backend:

- `fzf/` — Go binary built from a git tag (`pixi-build-go`)
- `xz/` — Autotools build with `autoreconf` (`pixi-build-autotools`)
- `redis/` — Plain Makefile build (`pixi-build-make`)
- `opus/` — Meson C library (`pixi-build-meson`)
- `detekt/` — Kotlin tool built via Gradle wrapper (`pixi-build-gradle`)
- `nodeserve/` — Node.js CLI wrapping `package.json` `bin` entries
- `nextjs-app/` — Next.js standalone build with a generated server launcher
- `buildifier/` — Bazel BUILD-file formatter built from a git tag (`pixi-build-bazel`)

## Building from source

The repo is a Cargo workspace; each backend is a binary crate under
[`crates/`](./crates). To build all backends locally with `cargo`:

```sh
cargo build --release --bins
```

To build the conda packages the same way CI does (using `rattler-build`):

```sh
pixi run -e build build-backend-packages osx-arm64
```

Substitute `linux-64`, `linux-aarch64`, or `win-64` as needed. Output lands
in `output/`.

## Layout

```
crates/                    # one Rust crate per backend
recipe/recipe.yaml         # rattler-build recipe for all backend packages
examples/                  # one minimal pixi.toml per backend
scripts/                   # version-bumping & upload helpers used by CI
```

## License

BSD-3-Clause. See [LICENSE](./LICENSE).
