# st0x.finance

Shared financial domain types for st0x services.

The `st0x-finance` crate provides checked types for stock symbols, fractional
shares, USD and USDC amounts, positive and non-negative values, and tagged
identifiers. Decimal arithmetic delegates to `rain-math-float` so offchain
calculations retain the same decimal semantics as Rain's onchain Float type.

The workspace also owns the `st0x-float-macro` and `st0x-float-serde` helper
packages. Keeping them beside `st0x-finance` ensures every consumer resolves the
same pinned `rain-math-float` source and therefore the same `Float` type.

## Usage

```toml
[dependencies]
st0x-finance = { git = "https://github.com/ST0x-Technology/st0x.finance", tag = "v0.2.0" }
```

The literal macros expand to paths in `rain-math-float` and
`alloy-primitives`, so direct macro consumers must declare all three packages.
The Float dependency must use the workspace's pinned revision to keep a single
`Float` type across the dependency graph:

```toml
[dependencies]
st0x-float-macro = { git = "https://github.com/ST0x-Technology/st0x.finance", tag = "v0.2.0" }
alloy-primitives = "=1.6.0"
rain-math-float = { version = "=0.1.7", git = "https://github.com/rainlanguage/rain.math.float", rev = "e226e5a27125e75208e3e709e1c5eee128bd8b3b" }
```

## Development

```sh
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets --all-features
nix develop -c cargo fmt --all -- --check
```
