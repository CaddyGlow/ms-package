# Malformed MSI Regression

`msi-null-column-type.msi` is the unchanged input from the local instrumented
package fuzz run dated 2026-10-05, iteration 935. It mutates the repository's
synthetic MSI fixture, not third-party production media. Upstream msi 0.10.0
panicked while unwrapping a required `_Columns` Type value. The local fork
must reject it with an error without unwinding or aborting.

SHA-256: `f2e7fdd74ef460e2f5a18f7746d8c345183efdcfc186db433305e90c064b5bf6`.
