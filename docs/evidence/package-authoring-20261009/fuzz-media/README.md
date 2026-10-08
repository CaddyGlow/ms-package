# Expanded authoring fuzz campaign — 2026-10-09

The final instrumented campaign passed 129 iterations for each target (`appx`, `msi`, `authoring`), with zero crashes and zero timeouts. Stateful authoring covers Stored/Deflate APPX edit sequences and embedded/external/loose/multiple MSI media using memory sinks/resolvers and an independent expected-payload model after every save. Eight explicit mode seeds ensure every media/compression selector is seeded. The receipt's source hashes match the final harness source.

The ordinary corpus/truncation/mutation test passed, as did eight additional deterministic 256-byte mode cases and strict locked fuzz Clippy. This is a bounded smoke campaign, not a security qualification or proof of absence of bugs.

The first build attempt failed because `bfd.h` was absent; its rejected build log/receipt are retained under `missing-bfd-header/`. The successful run used the shared default Rust development shell plus binutils/libunwind/xz development packages:

```sh
nix develop --impure --expr 'let dev = builtins.getFlake "path:/home/rick/projects-caddy/nix-dev-shells/default"; pkgs = dev.inputs.nixpkgs.legacyPackages.x86_64-linux; in pkgs.mkShell { inputsFrom = [ dev.devShells.x86_64-linux.default ]; packages = [ pkgs.binutils-unwrapped.dev pkgs.libunwind pkgs.xz ]; }' -c python3 scripts/fuzz-campaign.py --iterations 128 --output /data/cache/ms-package-fuzz-media-20261009-final
```

The full instrumented workspace remains outside the repository at the output path above. The campaign used published caddy-archive-core 0.2.1, ms-cabinet 0.1.3, and caddy-msi 0.10.1 from the committed fuzz lock.
