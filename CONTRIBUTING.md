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

For cross-compilation, we use different approaches depending on the target:

#### Linux Targets (musl)

For Linux musl targets, we use `cargo-zigbuild` which provides excellent cross-compilation support:

```bash
# Install Zig (required by cargo-zigbuild)
# Download from https://ziglang.org/download/ or use your package manager

# Install cargo-binstall for faster tool installation (optional but recommended)
curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash

# Install cargo-zigbuild (using binstall for speed)
cargo binstall cargo-zigbuild --no-confirm
# Or compile from source if you prefer:
# cargo install cargo-zigbuild --locked

# Build for Linux targets
cargo zigbuild --release --bin nexus --target x86_64-unknown-linux-musl
cargo zigbuild --release --bin nexus --target aarch64-unknown-linux-musl
```

#### macOS Targets

For macOS targets, use standard `cargo build` (no zigbuild needed):

```bash
# Build for macOS targets
cargo build --release --bin nexus --target x86_64-apple-darwin
cargo build --release --bin nexus --target aarch64-apple-darwin
```

Note: When building for a different macOS architecture than your host, you may need to install the appropriate Rust target:
```bash
rustup target add x86_64-apple-darwin    # If on Apple Silicon
rustup target add aarch64-apple-darwin   # If on Intel Mac
```
