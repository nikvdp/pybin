# pybin

`pybin` is a Python-focused self-extracting binary packer for uv-managed
projects.

It builds a one-file executable by combining:

1. an outer conda prefix for relocatability
2. an inner `uv-env` for normal uv project installs
3. `conda-pack` for freezing the full prefix
4. an in-repo [Warp](https://github.com/dgiagio/warp)-style runner and packer
   for the final SFX binary

The shipped `pybin` executable is self-contained: it is both the CLI you run and
the SFX stub used to build output binaries.

## Requirements

- host `conda`

You do not need host `uv` or host `conda-pack`. `pybin` installs both inside the
temporary conda build prefix it creates for each build.

## Build Topology

The accepted topology is:

1. create an outer conda env with Python, `uv`, and `conda-pack`
2. create an inner `uv-env` inside that conda prefix
3. run `uv sync --no-editable` with `UV_PROJECT_ENVIRONMENT=<outer>/uv-env`
4. `conda-pack` the outer prefix and unpack it into a staging dir
5. wrap the staged tree in a self-extracting runner
6. on first launch, extract to cache, run `conda-unpack`, then exec the app

This deliberately keeps uv as the project installer while relying on conda for
the relocatable host prefix.

## Rejected Shape

Do not sync directly into the outer conda prefix.

That topology was rejected because:

- default `uv sync` produces editable installs, which `conda-pack` rejects
- even with `--no-editable`, syncing into the outer prefix can clobber
  conda-managed packages like `setuptools`, `wheel`, and `packaging`
- a nested `uv-env` keeps the uv-managed runtime separate while still letting
  `conda-pack` relocate the whole outer host

## Usage

Build a project into a one-file executable:

```bash
pybin build /path/to/project
```

Choose an output path and keep build artifacts for inspection:

```bash
pybin build \
  ./fixtures/demo-app \
  --work-dir /tmp/pybin-demo-work \
  --output /tmp/demo-sfx
```

Run the produced binary:

```bash
/tmp/demo-sfx hello world
```

Override the extraction cache root:

```bash
PYBIN_CACHE_DIR=/tmp/pybin-cache /tmp/demo-sfx hello
```

By default, extracted runtimes are cached under the platform local data
directory:

- macOS: `~/Library/Application Support/pybin/packages/`
- Linux: `${XDG_DATA_HOME:-~/.local/share}/pybin/packages/`

Inspect whether a project is packable and show the resolved build plan:

```bash
pybin inspect /path/to/project
```

## Releases

Pushing a `v*` tag is intended to publish GitHub release assets for:

- macOS `x86_64` and `arm64`
- Linux `x86_64` and `arm64`

Each release asset contains one `pybin` executable for that target platform.

Windows package output is not supported yet. The Windows `pybin` executable can
compile, but bundles it produces currently include a Unix-style bash launcher
inside the archive. Windows extracts the bundle and then rejects that inner
launcher with `not a valid Win32 application` instead of starting the packaged
Python app. Windows support needs a native `.cmd` or `.exe` launcher inside the
bundle before Windows-built packages should be published or relied on.

## Fixture Validation

The repo includes a sample uv project in `fixtures/demo-app`.

Fast compile-and-unit coverage:

```bash
cargo test
```

Full end-to-end smoke:

```bash
cargo test --test e2e_build_fixture -- --ignored --nocapture
```

That ignored test builds the fixture, packs it into one binary, runs it, deletes
the extraction cache, and runs it again.

## Troubleshooting

- If a build step fails, inspect the logs under the chosen `--work-dir` or the
  auto-created `target/pybin/<timestamp>-<slug>/logs` directory.
- The packaged binary is same-platform only. `conda-pack` does not make a macOS
  build portable to Linux or vice versa.
- Windows-built packages are currently known not to run because their inner
  entry launcher is still Unix-style. Use Linux or macOS builders until pybin has
  a native Windows launcher.
- Host `conda` is required. `pybin inspect` is the quickest preflight check.
- Packaging expects `[project.scripts]` to define the entrypoint. If the project
  has multiple scripts, pass `--entrypoint <name>`.
- The inner environment is always built with `uv sync --no-editable`.
