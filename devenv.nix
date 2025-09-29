{ pkgs, ... }:
{
  packages = with pkgs; [
    rustup

    cargo-nextest
    cargo-insta
    cargo-make

    python3
  ];
}
