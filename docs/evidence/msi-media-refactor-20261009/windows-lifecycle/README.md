# MSI media backend refactor — final Windows qualification

All 18 MSI install/delete-every-payload/repair/uninstall lifecycles passed with exact hashes and uninstall absence checks, using the separate guest root `C:/Users/deploy/msi-media-backend-refactor-v014`. The 16 creation combinations cover x86/x64, per-user/per-machine, and embedded/external/loose/two external cabinets. Edited external and mixed embedded/external packages passed too. Historical `msi-matrix` guest evidence was preserved.

The final release-candidate artifacts (typed-reader integration, 255 UTF-16 Media.Cabinet guard, caddy-msi 0.10.2, ms-package 0.2.2, ms-cabinet 0.1.4) are byte-identical to all 63 baseline artifact hashes. The same exact payload/MSI/media bytes were used by these lifecycles; `final-matrix-equivalence.json` retains the equivalence receipt.

Both main and backend/FFI all-target test executables were cross-built with the shared rust-xwin shell and executed natively on Windows. All 28 executables exited zero without timeout; 191 tests passed. The final binaries include the UTF-16 schema guard and direct CAB codec error-category regression. `rust-tests/test-results.json`, per-executable stdout/stderr logs, and executable SHA-256s retain the native proof. Each child had a 120-second timeout and only that child would have been terminated.

The initial test harness collected null PowerShell Process.ExitCode values despite complete test output; the final harness captures the process handle before waiting and reran every executable to record actual exit codes. Those final results are the qualification basis. No MSI lifecycle or Rust test failed.

This evidence does not qualify major upgrades, arbitrary custom actions or database schemas, cabinet spanning, or signature trust. Windows VM cleanup remains with the root agent.
