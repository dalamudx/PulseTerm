# Locked terminal source patch

User approved the A1 read-only state-access patch in `docs/architecture/terminal-restore-evaluation.md`.

- Crate: **alacritty_terminal 0.26.0**, unchanged version and dependency declarations.
- Source: crates.io published archive; VCS commit **94e7c8874e526b1e67b349d9ba30ddf81669119e** (`alacritty_terminal` subdirectory).
- License: **Apache-2.0**, upstream `LICENSE-APACHE` and all copyright headers retained. Modification notice is at the added method.
- Archive SHA-256 and original file hashes: `alacritty_terminal.upstream.json`.
- Sole upstream change: `alacritty_terminal.patch`, adding `Term::inactive_grid(&self) -> &Grid<Cell>`. No parser, allocator, resize, buffer-swap or protocol behavior changes; no mutable accessor.
- Preserve all published source/tests and manifests without reformatting. Cache checksum/marker files are not source and were not imported.

`mise run terminal-vendor-check` verifies all 211 retained published files and hashes after removing precisely the documented getter insertion. A deliberate comment-change probe confirmed drift rejection without overwriting source. `.gitattributes` keeps vendor bytes unchanged on Windows checkout. Root Cargo workspace excludes vendor from first-party lints/format policies.

`mise run terminal-vendor-test` copies protected source outside the workspace and runs the original 132 unit, 45 reference and one documentation tests using `alacritty_terminal.test.lock` and `--locked`. Neither the product lock nor the original upstream lock is modified. Both tasks are registered in three-platform CI; only Linux execution has been observed. The test-only lock is not a production dependency update.

Upgrade procedure: obtain the published archive, verify its registry checksum, check whether upstream has the getter, update provenance hashes and minimal patch, run upstream tests and first-party continuation gates. Remove the patch when an upstream release provides the API. Never widen the patch to change semantics without a separate architecture review.

**No upstream PR has been submitted or accepted.** GTK macro crates remain on their original registry versions; dependency security/license gates are unchanged.
