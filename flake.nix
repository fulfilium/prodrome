{
  description = "Prodrome — a temporal, content-addressed event database with fulfillment-priority semantics";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    crane.url = "github:ipetkov/crane";
    # THE OWNER'S DESIGN SYSTEM, a Typst package (`@local/typst-design`):
    # the Typst package and the viewer take their look from it. Its source
    # only, so its own flake's inputs never enter this lock.
    typst-design = {
      url = "github:bmabsout/typst-design";
      flake = false;
    };
  };

  outputs = { self, nixpkgs, flake-utils, crane, typst-design }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        inherit (nixpkgs) lib;
        pkgs = import nixpkgs { inherit system; };

        # THE VERSION IS READ, NEVER TYPED TWICE. Two places to write a version
        # is one place to forget it, so this comes out of the workspace
        # manifest — the one file cargo also reads it from.
        version =
          let
            line = lib.findFirst (lib.hasPrefix "version = ")
              (throw "flake.nix: no version line in Cargo.toml")
              (lib.splitString "\n" (builtins.readFile ./Cargo.toml));
          in
          lib.removeSuffix "\"" (lib.removePrefix "version = \"" line);

        craneLib = crane.mkLib pkgs;

        # Panic messages carry the path of the source they come from. Crane
        # vendors crates into the store, so without this every artefact would
        # name the vendored sources — bytes in the wasm, and all of them in
        # its closure — where nixpkgs' vendoring left `/build/…`.
        remapVendorDir = ''
          for registry in "$cargoVendorDir"/*/; do
            export CARGO_BUILD_RUSTFLAGS="--remap-path-prefix=''${registry%/}=/build/cargo-vendor-dir"
          done
        '';

        # WHAT A BUILD IS ALLOWED TO SEE. Narrowed with `fileset` rather than
        # handed `./.`, so a README edit does not change a derivation's hash,
        # and narrowed per derivation, so each rebuilds only for its own
        # crates. Every member's manifest is always in: cargo loads the whole
        # workspace to build any part of it. `conformance/` is in the
        # workspace check because the tests read those vectors: a check that
        # cannot reach its evidence is a check that passes for the wrong
        # reason. The roadmap is not in any of them: it lives on the
        # `roadmap-data` branch, DATA the binary reads and not source the
        # binary is built from, so a todo appended to it rebuilds nothing,
        # and CI runs `prodrome verify` against that branch's checkout.
        rustSrc = paths: lib.fileset.toSource {
          root = ./.;
          fileset = lib.fileset.unions ([
            ./Cargo.toml
            ./Cargo.lock
            ./cli/Cargo.toml
            ./core/Cargo.toml
            ./github/Cargo.toml
            ./wasm/Cargo.toml
            ./wasm/exports/Cargo.toml
          ] ++ paths);
        };

        common = {
          inherit version;
          cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./Cargo.lock; };
          preBuild = remapVendorDir;
          strictDeps = true;
          # No debuginfo in the dev and test profiles: no check runs a
          # debugger or prints a backtrace, and a panic names its line
          # without it. Here, so that the deps are built to match.
          CARGO_PROFILE_DEV_DEBUG = "0";
        };

        # THE DEPENDENCY TREE, BUILT ONCE. `buildDepsOnly` compiles it against
        # a dummy of the workspace made from the manifests alone, so this
        # rebuilds when Cargo.toml or Cargo.lock does and never for a `.rs`
        # edit. Everything native below starts from its target directory.
        cargoArtifacts = craneLib.buildDepsOnly (common // {
          pname = "prodrome";
          src = rustSrc [ ./cli ./core ./github ./wasm ];
          doCheck = false;
          # One cargo invocation per consumer below: features unify across
          # the packages an invocation selects, so each needs its own build.
          buildPhaseCargoCommand = ''
            cargo check --locked --workspace --all-targets
            cargo test --locked --workspace --no-run
            cargo build --release --locked -p prodrome-cli
            cargo build --release --locked -p prodrome-github
          '';
        });

        # THE CORE, FOR THE BROWSER (`nix build .#prodrome-wasm`).
        #
        # `wasm/` compiled to WebAssembly, so a tab runs the same evaluator a
        # server does instead of a second reading of the same spec. Built
        # purely: the dependencies are vendored from `Cargo.lock`, nothing here
        # reaches the network, and the output is the two glues (`web/` for a
        # bundler, `nodejs/` for a script) beside the one `.wasm` each.
        #
        # THE wasm32 TARGET COMES FROM NIXPKGS. `rustc --print target-list` has
        # wasm32-unknown-unknown and `$(rustc --print sysroot)/lib/rustlib/`
        # carries its std, so there is no rust-overlay and no fenix input to
        # keep in step — one fewer flake input for a toolchain the pinned
        # nixpkgs already ships. `lld` is on the PATH because that rustc links
        # wasm with the system one rather than a bundled rust-lld.
        #
        # THE TWO wasm-bindgen HALVES ARE PINNED TOGETHER. The crate is
        # `=0.2.127` in wasm/Cargo.toml and the generator is
        # `wasm-bindgen-cli_0_2_127` here — they negotiate over a schema
        # version compiled into both, so a drift is a loud failure at bindgen
        # time, and pinning only one of them would make that failure a
        # `nix flake update` away.
        wasmArgs = profile: common // {
          src = rustSrc [ ./core/src ./wasm/src ./wasm/exports/src ];
          nativeBuildInputs = [ pkgs.lld ];
          doCheck = false;
          buildPhaseCargoCommand = ''
            cargo build --offline --frozen \
              --profile ${profile} --target wasm32-unknown-unknown -p prodrome-wasm
          '';
        };

        bindgen = pkgs.wasm-bindgen-cli_0_2_127;

        prodrome-wasm =
          let
            wasmArgs' = wasmArgs "wasm-release";
          in
          craneLib.mkCargoDerivation (wasmArgs' // {
            pname = "prodrome-wasm";
            cargoArtifacts = craneLib.buildDepsOnly (wasmArgs' // { pname = "prodrome-wasm"; });

            nativeBuildInputs = [
              pkgs.lld
              bindgen
              pkgs.binaryen # wasm-opt: -Os over what bindgen emits
            ];

            doInstallCargoArtifacts = false;

            # One glue per host. `web/` is the ES module an application imports
            # at runtime; `nodejs/` is a CommonJS glue that reads the file off
            # disk, which is what lets a test run the SAME module the browser
            # does under node.
            installPhaseCommand = ''
                wasm=target/wasm32-unknown-unknown/wasm-release/prodrome_wasm.wasm
                # The proposals rustc's wasm32-unknown-unknown baseline already
                # emits. wasm-opt's own default is an older baseline, so it
                # REFUSES a module using bulk memory rather than quietly
                # dropping it — named here so the set is a decision and not a
                # flag copied off an error message.
                features="--enable-bulk-memory --enable-bulk-memory-opt --enable-sign-ext"
                features="$features --enable-nontrapping-float-to-int --enable-mutable-globals"
                features="$features --enable-multivalue --enable-reference-types"
                for target in web nodejs; do
                  wasm-bindgen --target "$target" --out-dir "$out/$target" --out-name prodrome "$wasm"
                  # shellcheck disable=SC2086
                  wasm-opt -Os $features -o "$out/$target/prodrome_bg.wasm" "$out/$target/prodrome_bg.wasm"
                done
                echo "prodrome.wasm: $(stat -c %s "$out/web/prodrome_bg.wasm") bytes"
            '';

            meta = {
              description = "The Prodrome's core as WebAssembly, with the JS glue a page loads";
              license = with lib.licenses; [ mit asl20 ];
            };
          });

        # THE CORE FOR THE BROWSER, AS A CHECK: that it compiles for wasm32
        # and that bindgen makes both glues of it, in the dev profile, with no
        # wasm-opt. Fat LTO and one codegen unit buy the published module its
        # size and cost the check most of its time; they are the published
        # package's concern, and Pages builds that. No wasm32 artefact is
        # shared with `prodrome-wasm`: cargo keeps each profile's apart.
        prodrome-wasm-check = craneLib.mkCargoDerivation (wasmArgs "dev" // {
          pname = "prodrome-wasm-check";
          cargoArtifacts = craneLib.buildDepsOnly (wasmArgs "dev" // { pname = "prodrome-wasm-check"; });
          nativeBuildInputs = [ pkgs.lld bindgen ];
          doInstallCargoArtifacts = false;
          installPhaseCommand = ''
            for target in web nodejs; do
              wasm-bindgen --target "$target" --out-dir "$out/$target" --out-name prodrome \
                target/wasm32-unknown-unknown/debug/prodrome_wasm.wasm
            done
          '';
        });

        # TYPST FOR THE BROWSER (`nix build .#prodrome-typst-wasm`) — an
        # OPTIONAL EXTRA, outside the core's workspace and its lock file.
        #
        # `typst-wasm/` is its own cargo workspace with its own `Cargo.lock`,
        # so the typesetter's ~290 crates never enter the tree `prodrome-core`
        # and `prodrome-cli` are built from, and the sources above never see
        # it. Built the way `prodrome-wasm` is — vendored, offline, its
        # dependencies built once, the same pinned wasm-bindgen, `wasm-opt
        # -Os` — with the `web/` glue only: its one consumer is the viewer.
        # `SIZES` records the module raw and brotli-compressed, since the size
        # is what a first visit pays, and `web/typst_bg.wasm.br` is that
        # brotli file, quality 11.
        typstCommon = {
          version = "0.1.0";
          src = lib.fileset.toSource {
            root = ./typst-wasm;
            fileset = lib.fileset.unions [
              ./typst-wasm/Cargo.toml
              ./typst-wasm/Cargo.lock
              ./typst-wasm/src
            ];
          };
          cargoVendorDir = craneLib.vendorCargoDeps { cargoLock = ./typst-wasm/Cargo.lock; };
          preBuild = remapVendorDir;
          strictDeps = true;
        };

        # ONE BUILD PER FEATURE SET. The viewer's module is the crate's
        # default features: the compiler, `compile_html`, and the editor's
        # highlighting, completion and hover. A host that compiles views over
        # its state takes `prodrome-typst-wasm-views`: the compiler, the
        # persistent `Project` and `highlight`, which is the parser Typst
        # already holds; and its editor `prodrome-typst-wasm-editor`, the same
        # with typst-ide's completion and hover. All three share one
        # dependency build: typst-ide is the only crate a feature adds, and it
        # changes no other crate's features.
        typstWasmArgs = features: typstCommon // {
          nativeBuildInputs = [ pkgs.lld ];
          doCheck = false;
          buildPhaseCargoCommand = ''
            cargo build --offline --frozen ${features} \
              --profile wasm-release --target wasm32-unknown-unknown
          '';
        };

        typstWasmDeps = craneLib.buildDepsOnly (typstWasmArgs "" // { pname = "prodrome-typst-wasm"; });

        typstWasm = { pname, features, description }: craneLib.mkCargoDerivation (typstWasmArgs features // {
          inherit pname;
          cargoArtifacts = typstWasmDeps;

          nativeBuildInputs = [
            pkgs.lld
            pkgs.wasm-bindgen-cli_0_2_127
            pkgs.binaryen
            pkgs.brotli
          ];

          doInstallCargoArtifacts = false;

          installPhaseCommand = ''
            wasm=target/wasm32-unknown-unknown/wasm-release/prodrome_typst_wasm.wasm
            features="--enable-bulk-memory --enable-bulk-memory-opt --enable-sign-ext"
            features="$features --enable-nontrapping-float-to-int --enable-mutable-globals"
            features="$features --enable-multivalue --enable-reference-types"
            wasm-bindgen --target web --out-dir "$out/web" --out-name typst "$wasm"
            # shellcheck disable=SC2086
            wasm-opt -Os $features -o "$out/web/typst_bg.wasm" "$out/web/typst_bg.wasm"
            # PRECOMPRESSED, so a host serves `typst_bg.wasm.br` with
            # `Content-Encoding: br` and never compresses 20 MB per request,
            # nor settles for the quality an on-the-fly compressor can afford.
            brotli -q 11 -o "$out/web/typst_bg.wasm.br" "$out/web/typst_bg.wasm"
            raw=$(stat -c %s "$out/web/typst_bg.wasm")
            brotli=$(stat -c %s "$out/web/typst_bg.wasm.br")
            printf 'typst_bg.wasm\t%s bytes raw\t%s bytes brotli\n' "$raw" "$brotli" | tee "$out/SIZES"
          '';

          meta = {
            inherit description;
            license = with lib.licenses; [ mit asl20 ];
          };
        });

        prodrome-typst-wasm = typstWasm {
          pname = "prodrome-typst-wasm";
          features = "";
          description = "Typst 0.15.1 as WebAssembly: HTML export, highlighting and completion";
        };

        prodrome-typst-wasm-views = typstWasm {
          pname = "prodrome-typst-wasm-views";
          features = "--no-default-features --features highlight";
          description = "Typst 0.15.1 as WebAssembly for views: HTML export over a persistent world";
        };

        prodrome-typst-wasm-editor = typstWasm {
          pname = "prodrome-typst-wasm-editor";
          features = "--no-default-features --features highlight,ide";
          description = "Typst 0.15.1 as WebAssembly for a views editor: the views module and completion";
        };

        # THE TYPST PACKAGES ON A PACKAGE PATH, as `typst compile
        # --package-path` and typst-wasm find them: `local/<name>/<version>`,
        # the Prodrome's beside the design system it imports. Each is as
        # Typst installs it, its name, version and contents read from its own
        # manifest: its source without what the manifest excludes (examples,
        # tests, a gallery, a flake). The pictures of the Prodrome's examples
        # are not in its source at all: they are made from it.
        typstSrc = lib.fileset.toSource {
          root = ./typst;
          fileset = lib.fileset.difference ./typst (lib.fileset.fileFilter (file: file.hasExt "png") ./typst);
        };
        typstPackage = src:
          let
            manifest = (builtins.fromTOML (builtins.readFile "${src}/typst.toml")).package;
          in
          {
            name = "local/${manifest.name}/${manifest.version}";
            path = pkgs.runCommand "${manifest.name}-${manifest.version}" { } ''
              cp -r ${src} $out
              chmod -R u+w $out
              cd $out
              rm -rf ${lib.escapeShellArgs (manifest.exclude or [ ])}
            '';
          };
        typstPackages = pkgs.linkFarm "typst-packages" (map typstPackage [ typstSrc typst-design ]);

        # THE VIEWER (`nix build .#prodrome-viewer`): the static app — no
        # data in it — that folds a store with prodrome-wasm and typesets it
        # with prodrome-typst-wasm and the `typst/` package. An EXAMPLE host.
        # TypeScript typechecked by nixpkgs' `tsc` and bundled by its
        # `esbuild`, so nothing comes from npm; `viewer/build.sh` is the whole
        # recipe, and `viewer/assemble.sh` puts a store's objects beside it.
        viewer = { pname, wasm }: pkgs.stdenv.mkDerivation {
          inherit pname;
          version = "0.1.0";
          src = lib.fileset.toSource { root = ./.; fileset = ./viewer; };
          nativeBuildInputs = [ pkgs.esbuild pkgs.typescript pkgs.typst pkgs.jq ];
          buildPhase = ''
            runHook preBuild
            sh viewer/build.sh "$out" ${wasm}/web ${prodrome-typst-wasm}/web ${typstPackages} \
              ${pkgs.libertinus}/share/fonts ${pkgs.source-serif}/share/fonts
            runHook postBuild
          '';
          dontInstall = true;
          meta = {
            description = "A read-only Prodrome viewer: folded and typeset in the browser";
            license = with lib.licenses; [ mit asl20 ];
          };
        };

        prodrome-viewer = viewer { pname = "prodrome-viewer"; wasm = prodrome-wasm; };

        # Every Typst compilation here: no system font, so a picture is the
        # same on every machine, but the design system's faces (`faces` in
        # its `type.typ`: Source Serif 4, Crimson Pro, Source Sans 3), and
        # the packages on their path.
        typstCompile = ''
          typst compile --ignore-system-fonts --package-path ${typstPackages} \
            ${lib.concatMapStringsSep " " (font: "--font-path ${font}/share/fonts")
              [ pkgs.source-serif pkgs.crimson-pro pkgs.source-sans ]}'';

        # THE EXAMPLES AS PICTURES (`nix build .#prodrome-typst-examples`):
        # each example's one page as a PNG. `typst/examples/*.png` are these
        # pictures, committed so a reader sees the layouts without
        # compiling, and `checks.prodrome-typst` recomputes them: a picture
        # is a cache of a compilation, verified by compiling again (design
        # §6.2).
        prodrome-typst-examples = pkgs.runCommand "prodrome-typst-examples"
          { nativeBuildInputs = [ pkgs.typst ]; } ''
          mkdir -p $out
          cp -r ${typstSrc}/examples examples
          chmod -R u+w examples
          cd examples
          for doc in roadmap item; do
            ${typstCompile} --ppi 144 "$doc.typ" "$out/$doc.png"
          done
        '';

        # THE CORE AND THE CLI IN RELEASE, COMPILED ONCE. `prodrome-github`
        # links the same two crates with the same features, so both packages
        # start from this target directory: the CLI's binary is already built
        # and the mirror compiles only its own crate.
        cliArtifacts = craneLib.cargoBuild (common // {
          pname = "prodrome-cli";
          src = rustSrc [ ./core/src ./cli/src ];
          inherit cargoArtifacts;
          cargoExtraArgs = "--locked -p prodrome-cli";
          doCheck = false;
        });

        # THE BINARY (`nix build .#prodrome-cli`): what CI points at the
        # roadmap. Its tests ran in `prodrome-workspace`, not again here.
        prodrome-cli = craneLib.buildPackage (common // {
          pname = "prodrome-cli";
          src = rustSrc [ ./core/src ./cli/src ];
          cargoArtifacts = cliArtifacts;
          cargoExtraArgs = "--locked -p prodrome-cli";
          doCheck = false;
          meta = {
            description = "A command line over a Prodrome store";
            license = with lib.licenses; [ mit asl20 ];
            mainProgram = "prodrome";
          };
        });

        # THE GITHUB MIRROR (`nix build .#prodrome-github`), an integration kept
        # apart from the core and its CLI. Its fixture tests and laws ran in
        # `prodrome-workspace`.
        prodrome-github = craneLib.buildPackage (common // {
          pname = "prodrome-github";
          src = rustSrc [ ./core/src ./cli/src ./github/src ];
          cargoArtifacts = cliArtifacts;
          cargoExtraArgs = "--locked -p prodrome-github";
          doCheck = false;
          meta = {
            description = "Mirror GitHub issues into a Prodrome store";
            license = with lib.licenses; [ mit asl20 ];
            mainProgram = "prodrome-github";
          };
        });
      in
      {
        packages = {
          inherit prodrome-cli prodrome-github prodrome-wasm prodrome-typst-wasm prodrome-typst-wasm-views
            prodrome-typst-wasm-editor prodrome-typst-examples prodrome-viewer;
          default = prodrome-cli;
        };

        checks = {
          # A CHECK IS A VERDICT, NOT AN ARTEFACT — hence `touch $out`. What
          # `nix develop -c cargo …` runs by hand, in one target directory:
          # every crate's suites — SPEC §9 against `conformance/*.json` and
          # the laws over generated logs and DAGs — and clippy at `-D
          # warnings` over `--all-targets`, which is the tests too. The dev
          # profile: it compiles the suites in a fraction of release's time,
          # and they run no slower.
          prodrome-workspace = craneLib.mkCargoDerivation (common // {
            pname = "prodrome-workspace";
            src = rustSrc [ ./cli ./core ./github ./wasm ./conformance ];
            inherit cargoArtifacts;
            nativeBuildInputs = [ pkgs.rustfmt pkgs.clippy ];
            buildPhaseCargoCommand = ''
              cargo fmt --all --check
              cargo clippy --locked --workspace --all-targets -- -D warnings
              cargo test --locked --workspace
            '';
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
          });
          # typst-wasm's own tests (offsets, errors as values, packages, the
          # persistent world, highlighting, completion), natively: a separate
          # workspace, so a separate check. Clippy, pedantic, over every
          # feature set, since each must stand alone with no dead code; the
          # tests over the two that are built.
          prodrome-typst-wasm-tests = craneLib.mkCargoDerivation (typstCommon // {
            pname = "prodrome-typst-wasm-tests";
            cargoArtifacts = craneLib.buildDepsOnly (typstCommon // {
              pname = "prodrome-typst-wasm-tests";
              doCheck = false;
              buildPhaseCargoCommand = ''
                cargo check --release --locked --all-targets
                cargo test --release --locked --no-run
                cargo test --release --locked --no-run --no-default-features
              '';
            });
            nativeBuildInputs = [ pkgs.clippy ];
            buildPhaseCargoCommand = ''
              for features in "" oneshot highlight ide oneshot,highlight oneshot,ide highlight,ide \
                  oneshot,highlight,ide; do
                cargo clippy --release --locked --all-targets --no-default-features --features "$features" \
                  -- -D warnings -W clippy::pedantic
              done
              cargo test --release --locked
              cargo test --release --locked --no-default-features
            '';
            doInstallCargoArtifacts = false;
            installPhaseCommand = "touch $out";
          });
          # What a host recompiling its views pays, old export against new,
          # on the published modules (`typst-wasm/bench/views.mjs`): a
          # verdict that every compilation it times answers the HTML it
          # should, and the timings and memory as its output, `$out/BENCH`.
          prodrome-typst-wasm-bench = pkgs.runCommand "prodrome-typst-wasm-bench"
            { nativeBuildInputs = [ pkgs.nodejs ]; } ''
            mkdir -p $out
            node ${./typst-wasm/bench/views.mjs} ${prodrome-typst-wasm}/web ${prodrome-typst-wasm-views}/web \
              > $out/BENCH
            cat $out/BENCH
          '';
          # The Typst package compiles its examples with the typst the
          # pinned nixpkgs ships — 0.15.1, the version typst-wasm pins — to
          # PDF and to HTML, so a layout that breaks either target fails here,
          # and compiles its laws (`tests/laws.typ`), which fail the compile
          # when an assert does not hold. The committed pictures of the
          # examples must be what they render. The rasteriser's output is
          # not the same bytes on every machine (two builds of one
          # derivation differed in a few pixels, a normalised RMSE of
          # 0.0004), so the pictures are compared as pixels: the RMSE must
          # be at most 0.002, which three wholly changed pixels exceed.
          prodrome-typst = pkgs.runCommand "prodrome-typst-check"
            { nativeBuildInputs = [ pkgs.typst pkgs.imagemagick ]; } ''
            cp -r ${typstSrc} typst
            chmod -R u+w typst
            cd typst
            ${typstCompile} tests/laws.typ "$NIX_BUILD_TOP/laws.pdf"
            cd examples
            for doc in roadmap item; do
              ${typstCompile} "$doc.typ" "$doc.pdf"
              ${typstCompile} --features html --format html "$doc.typ" "$doc.html"
              # `compare` prints `<absolute> (<normalised>)` and exits 1 for
              # any difference; no figure (two sizes, a file that is no
              # picture) is a failure too.
              far=$(compare -metric RMSE ${prodrome-typst-examples}/$doc.png \
                ${./typst/examples}/$doc.png null: 2>&1 || true)
              far=$(printf '%s\n' "$far" | sed -nE 's/^[0-9.e+-]+ \(([0-9.e+-]+)\)$/\1/p')
              { [ -n "$far" ] && awk -v far="$far" 'BEGIN { exit !(far + 0 <= 0.002) }'; } || {
                echo "typst/examples/$doc.png is not what $doc.typ renders (RMSE ''${far:-none}):" >&2
                echo "  nix build .#prodrome-typst-examples && cp result/*.png typst/examples/" >&2
                exit 1
              }
            done
            grep -q 'href="#/todo/ship-the-viewer"' roadmap.html
            grep -q '<strong>the viewer</strong>' roadmap.html
            grep -q '<svg' item.html
            touch $out
          '';
          # The package compiles in typst-wasm's restricted project, whose
          # library has no `html` and no `plugin`, as in an open one: a
          # host may typeset a store with it for markup it does not trust.
          prodrome-typst-restricted = pkgs.runCommand "prodrome-typst-restricted"
            { nativeBuildInputs = [ pkgs.nodejs ]; } ''
            node ${typstSrc}/tests/restricted.mjs ${prodrome-typst-wasm}/web ${typstPackages} ${typstSrc}/examples
            touch $out
          '';
          # The wasm and the viewer as checks are the cheap wasm build: the
          # published pair differs only in the profile and in wasm-opt.
          prodrome-wasm = prodrome-wasm-check;
          prodrome-viewer = viewer { pname = "prodrome-viewer-check"; wasm = prodrome-wasm-check; };
          inherit prodrome-cli prodrome-github prodrome-typst-wasm prodrome-typst-wasm-views
            prodrome-typst-wasm-editor;
        };

        # `nix develop` — the toolchain the checks above use, plus the editor's
        # server. `lld` is what the wasm32 target links with: nixpkgs' rustc
        # carries that target's std but expects the linker on PATH rather than
        # bundling rust-lld, and without it a wasm build stops at
        # "linker `lld` not found".
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [ cargo rustc rustfmt clippy lld rust-analyzer ];
          RUST_BACKTRACE = "1";
        };
      });
}
