# MSI media lifecycle matrix — 2026-10-09

All 16 creation combinations passed Windows install, deletion of every installed payload, repair, and uninstall: x86/x64 × per-user/per-machine × embedded cabinet/external cabinet/loose media/two external nonspanning stored cabinets. Two additional x64 per-user canonical edits passed the same lifecycle: external cabinet and mixed embedded/external cabinets. Every installed and repaired payload matched its expected SHA-256; uninstall removed every payload.

Each matrix cell uses distinct explicit ProductCode, UpgradeCode, PackageCode, and component GUIDs. Edited packages retain their source product/component identities and explicitly change PackageCode. Split and mixed profiles verify both payloads through the additive additional-payload manifest argument.

Windows OS: 10.0.26200.0; msi.dll: 5.0.26100.7920. Tests ran sequentially on the dedicated disposable `package-authoring-20261008` Windows VM. No matrix or edited lifecycle failed. These checks do not qualify major upgrades, compressed/spanning cabinets, custom actions, PE version/language authoring, or arbitrary MSI schemas.

`evidence/<cell>/result.json` and all three verbose logs retain native results. `cells.json` and `edited-cells.json` retain expected payload/package hashes. `artifact-sha256.json` covers generated MSI/media/reference artifacts retained outside the repository at `/data/cache/ms-package-msi-matrix-artifacts`. `producer.rs`, its dependency lock, and the PowerShell harnesses retain reproduction inputs. The producer uses a local path dependency on the current ms-package source and published crates.io dependencies, including caddy-msi 0.10.1 and caddy-archive-core 0.2.0.
