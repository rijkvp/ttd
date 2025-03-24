{
  pkgs,
  ...
}:
{
  packages = [
    pkgs.cargo-edit
  ];

  languages.rust = {
    enable = true;
    channel = "stable";
  };

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy = {
      enable = true;
      settings = {
        allFeatures = true;
        denyWarnings = true;
      };
    };
  };
}
