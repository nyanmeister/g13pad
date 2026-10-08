# Source provenance

Driver baseline: [monnerat/g13](https://github.com/monnerat/g13), commit
`1e80eda4adc4fdd2d7ca1c2a963265ebab39d363`, dated 2020-09-09. Upstream's licensing
statement is preserved in `driver/UPSTREAM-README.md`; two helper files retain James
Fowler's MIT notice. The font8x8 table comes from Daniel Hepper's public-domain collection
and original credited VGA font sources. Existing per-file authorship comments are retained.

Imported driver fixes, previously carried as incremental patches in g13map:

1. `g13d-1e80eda-input-safety.patch`: retained pressed actions, safer parsing/LCD bounds.
2. `g13d-1e80eda-lcd-independent.patch`: independent USB acquisition with ordered input,
   manager ownership of dispatch and join-before-close lifecycle.
3. `g13d-1e80eda-unbind.patch`: KEY_RESERVED no-op and correct held-key release.
4. `g13d-1e80eda-stickmode.patch`: explicit textual mode-to-enum mapping.

The four patches remain in `tools/` as historical evidence. Apply none of them to the
consolidated source: they are already incorporated. The poll10 patch is superseded.
Before packaging changes, all 14 modified files matched the audit source byte-for-byte.

Consolidation changes: generated release identification in the build directory; optional
test dependencies and CTest proofs; FIFO mode 0660 with owner/type/identity validation;
safe FIFO removal; bounded 960-byte image loading; neutral text logo; portable service
and calibration integration. Original additions/modifications are GPL-3.0-or-later; imported
public-domain/MIT material retains its notices. Rust/editor history is retained from
the g13map repository through commit `3714581` and its predecessors.

The 0.2.9 panel SVG is original device artwork. The editor's outline/control positions
were retraced against the top-down `g13.png` from the upstream driver reference, then
reviewed at normal/minimum window sizes with key and thumb-button hit checks. That
reference photograph is not distributed as an application asset.

Historical deployment instructions are under `docs/history/`. They describe the old
private deployment and are not public-package installation instructions. The older
g13map checkout remains the rollback/source reference on the development machine.
