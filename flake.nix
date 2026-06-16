{
  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-26.05";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
  };

  outputs = {
    nixpkgs,
    rust-overlay,
    flake-utils,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (
      system: let
        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };
        cargoToml = pkgs.lib.importTOML ./Cargo.toml;
      in
        with pkgs; {
          packages.default = pkgs.rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;
            src = ./.;
            cargoLock.lockFile = ./Cargo.lock;

            doCheck = false;
          };

          devShells.default = mkShell {
            buildInputs = [
              cargo-watch
              sqlx-cli
              rust-analyzer
              (rust-bin.stable.latest.default.override {
                extensions = ["rust-src"];
              })
              tokio-console

              (writeShellScriptBin "ci" ''
                set -euo pipefail
                cargo fmt --all -- --check --color always
                cargo clippy --all-features --workspace -- -D warnings
                cargo test

                nix build
              '')
            ];

            PUSH_API_KEY = "dev-key";
            RUST_LOG = "info";
          };
        }
    );
}
