{ lib
, rustPlatform
, fetchFromGitHub
, pkg-config
}:

rustPlatform.buildRustPackage rec {
  pname = "jwst-cosmos";
  version = "0.1.0";

  src = ./..;

  cargoLock = {
    lockFile = ../Cargo.lock;
  };

  nativeBuildInputs = [
    pkg-config
  ];

  meta = with lib; {
    description = "JWST Space Image Browser and AI Image Generator TUI";
    homepage = "https://github.com/YOUR_USERNAME/jwst-cosmos";
    license = licenses.mit;
    maintainers = [];
    platforms = platforms.linux;
    mainProgram = "jwst-cosmos";
  };
}
