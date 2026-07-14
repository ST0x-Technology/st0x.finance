# st0x.finance

Shared financial domain types for st0x services.

The `st0x-finance` crate provides checked types for stock symbols, fractional
shares, USD and USDC amounts, positive and non-negative values, and tagged
identifiers. Decimal arithmetic delegates to `rain-math-float` so offchain
calculations retain the same decimal semantics as Rain's onchain Float type.

## Usage

```toml
[dependencies]
st0x-finance = { git = "https://github.com/ST0x-Technology/st0x.finance", tag = "v0.1.0" }
```

## Development

```sh
nix develop -c cargo test --workspace
nix develop -c cargo clippy --workspace --all-targets --all-features
nix develop -c cargo fmt --all --check
```
