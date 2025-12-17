{
  inputs = {
    flake-utils.url = "github:numtide/flake-utils";
    dream2nix = {
      url = "github:nix-community/dream2nix";
      inputs.purescript-overlay.follows = "";
      inputs.pyproject-nix.follows = "";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
  };
  outputs = inputs@{
    self,
    nixpkgs,
    flake-utils,
    dream2nix,
    rust-overlay,
    ...
  }:
    flake-utils.lib.eachDefaultSystem (system: let
      pkgs = import nixpkgs {
        inherit system;
        overlays = [ rust-overlay.overlays.default ];
      };

      # Parse rust-toolchain.toml for metadata
      rustToolchainToml = builtins.fromTOML (builtins.readFile ./rust-toolchain.toml);
      rustChannel = rustToolchainToml.toolchain.channel;
      rustTargets = rustToolchainToml.toolchain.targets or [];
      rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      # add local environment nodejs packages to the shell
      localEnvPackageJson = builtins.fromJSON (builtins.readFile ./local-environment/package.json);
      localEnvPkgs = dream2nix.lib.evalModules {
        packageSets.nixpkgs = nixpkgs.legacyPackages.${system};
        modules = [
          {
            imports = [
              dream2nix.modules.dream2nix.nodejs-package-lock-v3
              dream2nix.modules.dream2nix.nodejs-granular-v3
            ];
            inherit (localEnvPackageJson) name version;
            mkDerivation.src = ./local-environment;
            nodejs-package-lock-v3.packageLockFile = ./local-environment/package-lock.json;
          }
        ];
      };

      # User facing devshell packages
      devshellPackages = with pkgs; [
        earthly
        rustToolchain
        clang
        nodejs
        pnpm
        kubectl
        just
      ];

      # Filter out rustToolchain from general list (shown separately)
      nonRustPackages = builtins.filter (pkg: pkg != rustToolchain) devshellPackages;
      
      versionInfo = pkgs.lib.concatStringsSep "\\n" (
        builtins.filter (x: x != "") (
          map (
            pkg: let
              name = pkg.pname or pkg.name or "unknown";
              version = pkg.version or pkg.meta.version or null;
              description = pkg.meta.description or null;
              descStr =
                if description != null
                then " - ${description}"
                else "";
            in
              if version != null
              then "  ${name}: ${version}${descStr}"
              else ""
          )
          nonRustPackages
        )
      );

      # Generate version info for npm devDependencies
      devDepsInfo = pkgs.lib.concatStringsSep "\\n" (
        pkgs.lib.mapAttrsToList (name: version: "  ${name}: ${version}")
        (localEnvPackageJson.devDependencies or {})
      );

      rustTargetsInfo = pkgs.lib.concatStringsSep ", " rustTargets;

      devshellInfoScript = pkgs.writeShellScriptBin "devshell-info" ''
        echo "🔧 Devshell packages:"
        echo -e "${versionInfo}"
        echo ""
        echo "🦀 Rust toolchain (channel ${rustChannel}):"
        echo "  rustc: $(rustc --version 2>/dev/null | cut -d' ' -f2)"
        echo "  cargo: $(cargo --version 2>/dev/null | cut -d' ' -f2)"
        echo "  rustfmt: $(rustfmt --version 2>/dev/null | cut -d' ' -f2)"
        echo "  clippy: $(cargo-clippy --version 2>/dev/null | cut -d' ' -f2)"
        echo "  rust-analyzer: $(rust-analyzer --version 2>/dev/null | cut -d' ' -f2)"
        echo "  targets: ${rustTargetsInfo}"
        echo ""
        echo "📦 npm devDependencies (${localEnvPackageJson.name} ${localEnvPackageJson.version}):"
        echo -e "${devDepsInfo}"
      '';
    in {
      packages.default = devshellPackages;
      devShells.default = pkgs.mkShell {
        packages = devshellPackages ++ [devshellInfoScript];
        buildInputs = with pkgs; [
          pkg-config
          zlib
          libclang
        ];
        WASM_BUILD_STD = "0";
        LIBCLANG_PATH = "${pkgs.llvmPackages.libclang.lib}/lib";
        PROTOC = "${pkgs.protobuf}/bin/protoc";
        CRATE_CC_NO_DEFAULTS = "1";
        OPENSSL_NO_VENDOR = "1";
        OPENSSL_DIR = "${pkgs.openssl.dev}";
        OPENSSL_INCLUDE_DIR = "${pkgs.openssl.dev}/include";
        OPENSSL_LIB_DIR = "${pkgs.openssl.out}/lib";
        ROCKSDB_LIB_DIR = "${pkgs.rocksdb}/lib";
        shellHook = ''
          export PATH="${localEnvPkgs}/lib/node_modules/.bin:$PATH"
          devshell-info
        '';
      };
    });
}
