{pkgs, ...}: {
  packages = with pkgs; [
    rustup

    cargo-nextest
    cargo-insta

    python3
  ];
}
