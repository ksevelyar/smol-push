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

            DATABASE_URL = "postgres://postgres:postgres@localhost:5432/smol_push";
            PUSH_API_KEY = "dev-key";
            ANDROID_API_KEY = "test-key";
            ANDROID_ADDRESS = "http://127.0.0.1:9099";
            MAX_QUEUED_PUSHES = "10000";
            MAX_CONNECTIONS_PER_PROVIDER = "1";
            MAX_CONCURRENT_STREAMS = "100";
            MAX_PUSHES_PER_CONNECTION_PER_SECOND = "100";
            MAX_RETRY_ATTEMPTS = "3";
            RETRY_BASE_DELAY_MS = "1000";
            RETRY_MAX_DELAY_MS = "60000";
            RUST_LOG = "info";
          };
        }
    );
}
