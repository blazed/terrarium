{
  pkgs,
  ...
}:
{
  languages.rust = {
    enable = true;
    channel = "stable";
    components = [
      "cargo"
      "clippy"
      "llvm-tools-preview"
      "rust-analyzer"
      "rustc"
      "rustfmt"
    ];
  };

  languages.nix.enable = true;

  packages = with pkgs; [
    actionlint
    cargo-deny
    grpcurl
    nixfmt
    postgresql
    python3
    ruff
    shellcheck
    shfmt
    sqlx-cli
  ];

  enterShell = ''
    echo "Terrarium"
  '';
}
