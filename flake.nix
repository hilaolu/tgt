{
  inputs = {
    rust-overlay.url = "github:oxalica/rust-overlay";
    flake-utils.url = "github:numtide/flake-utils";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  outputs = inputs:
    with inputs;
    flake-utils.lib.eachDefaultSystem (system:
      let

        overlays = [(import rust-overlay)];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = let rust = pkgs.rust-bin;
        in if builtins.pathExists ./rust-toolchain.toml then
          rust.fromRustupToolchainFile ./rust-toolchain.toml
        else if builtins.pathExists ./rust-toolchain then
          rust.fromRustupToolchainFile ./rust-toolchain
        else
          rust.stable.latest.default.override {
            extensions = [ "rust-src" "rustfmt" ];
          };

      in rec {
        devShells = {
          default = pkgs.mkShell {

            nativeBuildInputs=with pkgs;[pkg-config];

            
            buildInputs = with pkgs; [
              rustToolchain
              openssl
              pkg-config
              cargo-deny
              cargo-edit
              cargo-watch
              rust-analyzer
            ];

            env = {
              # Required by rust-analyzer
              RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
            };
          };

        };

      });
}
