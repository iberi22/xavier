# Headless Xavier server: `--no-default-features --features headless`, binary `xavier` only.
{
  lib,
  rustPlatform,
  pkg-config,
  protobuf,
  version,
}:

let
  root = ../..;
in
rustPlatform.buildRustPackage {
  pname = "xavier-server";
  inherit version;

  src = lib.fileset.toSource {
    inherit root;
    fileset = lib.fileset.unions [
      (root + "/Cargo.toml")
      (root + "/Cargo.lock")
      (root + "/LICENSE")
      (root + "/src")
      (root + "/benches")
      (root + "/tests")
      (root + "/code-graph")
      (root + "/codegraph-types")
      (root + "/crates")
      (root + "/vendor/maloca-core")
      (root + "/panel-ui/src-tauri")
      (root + "/packaging/linux")
    ];
  };

  cargoLock.lockFile = root + "/Cargo.lock";

  buildNoDefaultFeatures = true;
  buildFeatures = [ "headless" ];
  cargoBuildFlags = [
    "--package"
    "xavier"
    "--bin"
    "xavier"
  ];
  # The test suite runs in CI under `ci-safe`; the package build only proves the binary.
  doCheck = false;

  nativeBuildInputs = [
    pkg-config
    protobuf
  ];

  meta = {
    description = "Xavier memory runtime for AI agents (headless server)";
    homepage = "https://github.com/iberi22/xavier";
    license = lib.licenses.agpl3Only;
    mainProgram = "xavier";
    platforms = lib.platforms.linux;
  };
}
