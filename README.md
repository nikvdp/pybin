# pybin

`pybin` is a Python-focused self-extracting binary packer for uv-managed
projects.

It builds a one-file executable by combining:

1. an outer conda prefix for relocatability
2. an inner `uv-env` for normal uv project installs
3. `conda-pack` for freezing the full prefix
4. an in-repo Warp-style runner and packer for the final SFX binary

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
WARP_CACHE_DIR=/tmp/pybin-cache /tmp/demo-sfx hello
```

Inspect the resolved build plan before packaging:

```bash
pybin inspect /path/to/project
```

Check project readiness and host prerequisites:

```bash
pybin doctor /path/to/project
```

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
- Host `conda` is required. `pybin doctor` is the quickest preflight check.
- Packaging expects `[project.scripts]` to define the entrypoint. If the project
  has multiple scripts, pass `--entrypoint <name>`.
- The inner environment is always built with `uv sync --no-editable`.
