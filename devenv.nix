{ pkgs, ... }:
{
  packages = with pkgs; [
    rustup
    taplo

    cargo-nextest
    cargo-insta
    cargo-make

    python3
    openapi-generator-cli
  ];
}
