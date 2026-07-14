{
  description = "Development environment for st0x.finance";

  inputs = {
    rainix.url = "github:rainlanguage/rainix";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      flake-utils,
      rainix,
      ...
    }:
    flake-utils.lib.eachSystem [ "aarch64-darwin" "x86_64-linux" ] (
      system:
      let
        pkgs = rainix.pkgs.${system};
      in
      {
        devShells.default = pkgs.mkShell {
          inherit (rainix.devShells.${system}.default) shellHook;
          inputsFrom = [ rainix.devShells.${system}.default ];
        };
      }
    );
}
