# Third-party notices

Forge is MIT licensed (see `LICENSE`). It includes the components below, whose licences also apply.

## Open CASCADE Technology 8.0.1

- Licence: GNU LGPL 2.1 with the Open CASCADE exception 1.0
  (`licenses/OpenCASCADE-LGPL-2.1.txt`, `licenses/OpenCASCADE-LGPL-exception.txt`).
- Source: https://github.com/Open-Cascade-SAS/OCCT (tag `V8_0_1`).
- Forge links it statically through the cadrum crate. Forge's complete source is available under the
  MIT licence, so you can rebuild Forge against a modified Open CASCADE: replace the library that
  cadrum downloads (or build cadrum from Open CASCADE source) and run `cargo build --release`.

## cadrum 0.9

- Licence: MIT (`licenses/cadrum-MIT.txt`).
- Source: https://crates.io/crates/cadrum

## Other Rust and npm packages

Every other dependency is listed with its licence in `Cargo.lock` and `package-lock.json`.
Run `cargo tree -e normal --format "{p} {l}"` or `npx license-checker` to print them.
