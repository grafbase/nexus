# Contributing

Thank you for considering contributing to Nexus!

## Submitting Issues

When submitting an issue, please select the most appropriate issue template and follow the instructions within. If none of the templates fit your scenario, open a blank issue and attempt to provide as many details as possible.

## Pull Requests

- For small changes - Feel free to submit small PRs directly and/or consult us on Discord beforehand
- For large changes - Before submitting/starting work on a large PR it would be best to open an issue or consult us on Discord so we can discuss the changes and why they're needed.

We're very excited that you've chosen to contribute to our open-source codebase and hope you have a great experience!

## Building

### Local Development

For local development, you can build Nexus with:

```bash
cargo build --bin nexus
```

### Cross-Compilation

For cross-compilation to different targets (especially musl targets), we use `cargo-zigbuild` which provides better cross-compilation support:

```bash
# Install cargo-binstall for faster tool installation (optional but recommended)
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash

# Install cargo-zigbuild (using binstall for speed)
cargo binstall cargo-zigbuild --no-confirm
# Or compile from source if you prefer:
# cargo install cargo-zigbuild --locked

# Build for a specific target
cargo zigbuild --release --bin nexus --target x86_64-unknown-linux-musl
```

Supported targets include:
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`
- `x86_64-apple-darwin`
- `aarch64-apple-darwin`
