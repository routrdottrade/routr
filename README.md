# routr

Source of [routr](https://routr.trade), the launch venue on NEAR (`routr.near`).
Documentation: [routr.trade](https://routr.trade).

## Verify a deployed contract

The `contracts` workflow builds each crate with `cargo near build reproducible-wasm` (the pinned image is in each
`Cargo.toml`) and prints the sha256 (base58) of every wasm. Compare it with the account's code hash. Each wasm names
its commit in its NEP-330 metadata (`contract_source_metadata`): rebuild that commit to get the same hash.

Licence: Business Source License 1.1 (`LICENSE`).
