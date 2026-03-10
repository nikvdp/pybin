# pybin

`pybin` is a Python-focused self-extracting binary packer.

The intended build shape is:

1. create an outer conda environment
2. create an inner uv-managed environment inside that conda prefix
3. sync the project into the inner env in non-editable mode
4. pack the outer prefix into a self-extracting executable

This repository is in early bootstrap. The CLI shape is present, but the actual
build pipeline and runtime packer are still being implemented.
