{
  pkgs,
  ...
}:
{
  packages = [
    pkgs.cargo-edit
  ];

  languages.rust.enable = true;

  git-hooks.hooks = {
    rustfmt.enable = true;
    clippy.enable = true;
    clippy.settings.allFeatures = true;
  };
}
