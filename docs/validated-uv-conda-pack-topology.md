# Validated uv + conda-pack Topology

keywords:: uv, conda, conda-pack, topology, architecture, nested venv, relocatable, proof

## Goal

Package a uv-managed Python application as a single self-extracting executable while
using conda only as the relocatable outer substrate.

The project question was not whether Python virtual environments are portable in
general. The question was which concrete topology works when combined with
`conda-pack` and a Warp-style self-extracting runner.

## Tool versions used in the spike

- `conda 24.11.2`
- `uv 0.9.7`
- `conda-pack 0.9.1` (installed temporarily for the experiment)

Date of experiment: 2026-03-10

## Tested cases

### Case 1: Activated conda env + plain `uv sync`

Setup:

- create a conda env with Python
- activate it
- run `uv sync` from a uv project

Observed result:

- `uv` still created the project-local `.venv`
- the active conda env was not used as the project environment

Conclusion:

- plain activated-conda `uv sync` is not enough
- the target uv environment path must be set explicitly

## Case 2: `UV_PROJECT_ENVIRONMENT=$CONDA_PREFIX` with default editable sync

Setup:

- create an outer conda env
- set `UV_PROJECT_ENVIRONMENT` to that outer prefix
- run `uv sync`

Observed result:

- the project installed into the outer conda prefix
- `conda-pack` refused to package the result because editable packages were present

Representative failure:

```text
CondaPackError: Cannot pack an environment with editable packages installed
```

Conclusion:

- distributable builds must not rely on editable installs
- packaging mode must use `uv sync --no-editable` or an equivalent finalization step

## Case 3: `UV_PROJECT_ENVIRONMENT=$CONDA_PREFIX` with `--no-editable`

Setup:

- create an outer conda env
- set `UV_PROJECT_ENVIRONMENT` to that outer prefix
- run `uv sync --no-editable`

Observed result:

- the project installed into the outer conda prefix
- `conda-pack` still refused to package the result
- uv had overwritten conda-managed files from `setuptools`, `wheel`, and `packaging`

Representative failure:

```text
CondaPackError:
Files managed by conda were found to have been deleted/overwritten ...
This is usually due to `pip` uninstalling or clobbering conda managed files
```

Conclusion:

- directly syncing into the outer conda prefix is not a viable design
- even in non-editable mode, uv collides with conda-managed packaging files

## Case 4: Inner uv environment inside the conda prefix

Setup:

- create an outer conda env
- choose an inner uv-managed env path under that prefix, for example
  `$CONDA_PREFIX/uv-env`
- set `UV_PROJECT_ENVIRONMENT` to that inner path
- run `uv sync --no-editable`
- package the outer conda prefix with `conda-pack`
- unpack elsewhere
- run `conda-unpack`
- execute the inner environment's console script

Observed result:

- the inner uv environment was created successfully
- the outer conda prefix remained packable
- the packed environment unpacked successfully
- `conda-unpack` completed successfully
- the console entrypoint from the inner uv env ran successfully after relocation

Conclusion:

- this is the accepted topology

## Accepted design

`pybin` should use this layout:

1. create an outer conda env
2. let conda manage the requested Python version
3. create or sync an inner uv-managed environment under the outer prefix
4. run `uv sync --no-editable`
5. pack the outer conda prefix
6. on first extraction, run `conda-unpack`
7. execute the chosen entrypoint from the inner uv-managed environment

## Design rules derived from the spike

- Do not rely on plain activated-conda `uv sync`.
- Do not sync directly into the outer conda prefix.
- Do not ship editable installs in final artifacts.
- Keep the inner uv environment inside the outer conda prefix so it is included in
  the packed payload.
- Treat the outer conda env as the relocatable host and the inner uv env as the
  user-facing Python runtime.

## Implications for implementation

- `pybin` should use uv project flows, not `uv pip install`, for the main path.
- `pybin` should model the inner uv environment path explicitly in its build plan.
- runtime launch should execute the inner env entrypoint after the outer prefix has
  been unpacked and `conda-unpack` has run.

## Remaining questions

- exact entrypoint selection UX for multi-script projects
- platform coverage beyond the current host architecture
- how much of the old Warp behavior should be mirrored versus simplified
