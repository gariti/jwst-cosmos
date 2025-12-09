{
  description = "JWST Space Image Browser and AI Image Generator TUI";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        overlays = [ (import rust-overlay) ];
        pkgs = import nixpkgs {
          inherit system overlays;
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
        };

        jwst-cosmos = pkgs.rustPlatform.buildRustPackage {
          pname = "jwst-cosmos";
          version = "0.1.0";

          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          meta = with pkgs.lib; {
            description = "JWST Space Image Browser and AI Image Generator TUI";
            homepage = "https://github.com/YOUR_USERNAME/jwst-cosmos";
            license = licenses.mit;
            maintainers = [];
            platforms = platforms.linux;
            mainProgram = "jwst-cosmos";
          };
        };
      in
      {
        packages = {
          default = jwst-cosmos;
          jwst-cosmos = jwst-cosmos;
        };

        apps.default = flake-utils.lib.mkApp {
          drv = jwst-cosmos;
        };

        devShells.default = pkgs.mkShell {
          buildInputs = with pkgs; [
            rustToolchain
            rust-analyzer
            cargo-watch
            cargo-edit
          ];

          RUST_SRC_PATH = "${rustToolchain}/lib/rustlib/src/rust/library";
        };
      }
    );
}
