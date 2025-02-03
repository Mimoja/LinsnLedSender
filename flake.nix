{
  inputs = {
    nixpkgs.url = "nixpkgs/nixos-unstable";

    flake-utils.url = "github:numtide/flake-utils";
    flake-compat.url = "github:edolstra/flake-compat";

    naersk.url = "github:nix-community/naersk";
    naersk.inputs.nixpkgs.follows = "nixpkgs";
  };
  outputs =
    {
      nixpkgs,
      flake-utils,
      naersk,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};
      in
      {
        devShell = pkgs.mkShell {
          nativeBuildInputs = with pkgs; [
            cargo
            rustc

            rustfmt
            clippy
            rust-analyzer

            rustPlatform.bindgenHook
            pkg-config
            gobject-introspection
            gst_all_1.gstreamer
            gst_all_1.gst-plugins-base
          ];
        };

        formatter = pkgs.nixfmt-rfc-style;
      }
    );
}
