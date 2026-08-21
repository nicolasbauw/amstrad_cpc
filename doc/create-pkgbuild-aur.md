# Creating a PKGBUILD for the AUR

Original draft generated from a web search; reviewed and rewritten here
against ByteBox's actual `Cargo.toml`, `packaging/bytebox.desktop`, and
build setup — a few things in the generic version didn't hold up (missing
`build()`/`package()` functions, no checksums, generic paths instead of
this project's own).

## Step 1 — Prerequisites

```sh
sudo pacman -S base-devel git
```

`base-devel` provides `makepkg` and the standard build toolchain (`gcc`,
`make`, `fakeroot`...); `git` is needed both to clone the empty AUR
repository later and, in the PKGBUILD itself, to fetch ByteBox's own
source.

## Step 2 — Anatomy of a PKGBUILD

A `PKGBUILD` is a shell script `makepkg` sources to build and package
software. The metadata fields are plain shell variables:

| Variable      | Meaning                                                              |
|---------------|-----------------------------------------------------------------------|
| `pkgname`     | Package name, as it will appear on the AUR.                          |
| `pkgver`      | Package version (e.g. `2.0.0`).                                       |
| `pkgrel`      | Release number for this `pkgver`; starts at `1`, bumped on packaging-only changes (no source change). |
| `pkgdesc`     | One-line description.                                                 |
| `arch`        | Target architectures, e.g. `('x86_64')` — see note below.            |
| `url`         | Project homepage.                                                     |
| `license`     | e.g. `('MIT')`.                                                       |
| `depends`     | Packages required at **runtime**.                                     |
| `makedepends` | Packages required only to **build** (not needed once installed).      |
| `source`      | Where to fetch the source from.                                       |
| `sha256sums`  | One checksum per `source` entry, or `'SKIP'` for VCS sources.         |

The generic version of this doc stopped there — but a PKGBUILD isn't just
metadata. Two shell functions do the actual work, and without them
`makepkg` has nothing to run:

- **`build()`** — compiles the source (here, `cargo build`).
- **`package()`** — copies the *built* files into `$pkgdir`, staged exactly
  as they should land on the end user's filesystem (e.g. the binary under
  `$pkgdir/usr/bin/`).

`arch=('x86_64')`, not `('any')`: ByteBox is a compiled native binary
(SDL2/wgpu), not an interpreted script or pure-data package — `any` is only
for packages with no architecture-specific code.

## Step 3 — Two working PKGBUILDs for ByteBox

Both live in the repo, ready to copy — not just examples here. Both have
been built end to end with `makepkg` (`--nodeps`, since the sandbox they
were tested in isn't Arch itself): package produced, binary extracted and
launched successfully, for each.

- **`packaging/PKGBUILD`** — the "official" package, `bytebox`, built from
  the tagged release (`2.0.0` as of this writing). This is the one to
  submit to the AUR as the primary package: fixed `pkgver`, a real
  `sha256sums` on the release tarball, no dependency on `git` at build
  time.
- **`packaging/PKGBUILD-git`** — `bytebox-git`, tracking the latest commit
  on `master` instead of a tagged release. Conventional AUR pattern for
  offering a "bleeding edge" alongside the stable package: `provides`/
  `conflicts=('bytebox')` so the two can't be installed side by side, and a
  `pkgver()` function since there's no fixed version to declare up front.

```sh
# packaging/PKGBUILD — the release package
# Maintainer: Nicolas BAUW <nbauw@hotmail.com>
pkgname=bytebox
pkgver=2.0.0
pkgrel=1
pkgdesc="Amstrad CPC 6128 emulator"
arch=('x86_64')
url="https://github.com/nicolasbauw/amstrad_cpc"
license=('MIT')
depends=('sdl2')
makedepends=('rust')
source=("$pkgver.tar.gz::https://github.com/nicolasbauw/amstrad_cpc/archive/refs/tags/$pkgver.tar.gz")
sha256sums=('70a8bbd18e1d8b165cbcf6a499fecb31876f6d2a59ab837a9cf174dabbb72c2b')

build() {
  # Archive source, not a git clone: the extracted directory keeps the
  # GitHub repo's name (amstrad_cpc), not the package's (bytebox).
  cd "amstrad_cpc-$pkgver"
  # --profile dist (not --release): a slower-to-compile but smaller/faster
  # profile reserved for distributed binaries (LTO, single codegen unit —
  # see the root Cargo.toml).
  #
  # makepkg.conf's LTOFLAGS ("-flto=auto") ends up in CFLAGS/CXXFLAGS: with
  # this profile's own Rust-side LTO (`dist` = `lto = "thin"`), cargo
  # detects both and auto-enables cross-language LTO (-C linker-plugin-lto)
  # for the whole build. That mode needs clang (LLVM bitcode) to compile
  # ring/zstd-sys's C code, not gcc (the default here) — with gcc, it
  # silently produces object files the final Rust link can't resolve
  # ("undefined symbol: ring_core_..."). Confirmed by reproducing and
  # fixing it this way before writing this line. Stripped, not just
  # unset, so unrelated -O2/-march flags survive.
  export CFLAGS="${CFLAGS//-flto=auto/}"
  export CXXFLAGS="${CXXFLAGS//-flto=auto/}"
  export LDFLAGS="${LDFLAGS//-flto=auto/}"
  cargo build --profile dist --locked
}

package() {
  cd "amstrad_cpc-$pkgver"
  install -Dm755 target/dist/bytebox "$pkgdir/usr/bin/bytebox"
  install -Dm644 packaging/bytebox.desktop \
    "$pkgdir/usr/share/applications/bytebox.desktop"
  install -Dm644 assets/bytebox_icon.png \
    "$pkgdir/usr/share/icons/hicolor/256x256/apps/bytebox.png"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
```

`packaging/PKGBUILD-git` is the same `build()`/`package()` (same LTO
workaround included), with the metadata that differs for a VCS package:

```sh
# packaging/PKGBUILD-git — the differences from the release one above
pkgname=bytebox-git
pkgver=r1.abcdef0  # placeholder — recomputed by pkgver() below
makedepends=('rust' 'git')
provides=('bytebox')
conflicts=('bytebox')
source=("$pkgname::git+$url.git#branch=master")
sha256sums=('SKIP')  # VCS source: integrity comes from git itself

pkgver() {
  cd "$pkgname"
  # Nothing tagged as of the commit this builds from would fall back to
  # "r<commit count>.<short hash>", the standard scheme for AUR -git
  # packages without upstream tags — kept even now that 2.0.0 exists, so
  # this package still tracks every commit past it, not just tags.
  printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}
# build()/package(): identical to packaging/PKGBUILD, except cd "$pkgname"
# instead of cd "amstrad_cpc-$pkgver" (a git clone keeps the name given to
# the source= entry, unlike a tarball extracting under the repo's own name).
```

Notes specific to this project:

- **The LTO/linker bug above was the one real surprise** — everything else
  in these PKGBUILDs worked on the first or second try; this one took
  several rounds of elimination (LDFLAGS's `--as-needed`? the `lld` linker
  itself? before landing on the actual cause, makepkg's auto-added
  `-flto=auto` triggering cross-language LTO). Worth knowing if a *future*
  dependency reintroduces a similar "undefined symbol" failure that only
  reproduces under `makepkg`, never under a plain `cargo build`: check
  `cargo build -vv` for `-C linker-plugin-lto` in the compile command
  before suspecting anything else.
- **`makedepends=('rust' ...)`, not `'cargo'`** — on Arch, the standalone
  `cargo` package is gone; it's now provided virtually by `rust`
  (`pacman -Qi rust` shows `Provides: cargo rustfmt`, and `Conflicts With:
  cargo`). `makedepends=('cargo')` still resolves correctly through that
  virtual `Provides`, but names the actual package explicitly instead of
  relying on it.
- **`depends=('sdl2')` only** — no `SDL2_ttf`: an earlier design used it for
  the machine-status window's text, replaced by egui (which ships its own
  font, no system font dependency either). SDL2 itself is dynamically
  linked (`Cargo.toml` doesn't enable the `sdl2` crate's `bundled` feature),
  so the `sdl2` package covers both build-time headers and the runtime
  library on Arch (no separate `-dev` split there).
- **No icon/keyboard-image install step needed beyond the one line above**:
  both the window icon and the virtual keyboard (F7) illustration are
  compiled directly into the binary (`include_bytes!`), not read from disk
  at runtime — only the *launcher* icon (looked up by name through the icon
  theme, independent of the running process) still needs installing.
- **ROMs are deliberately never packaged** — their legal status hasn't been
  settled by Amstrad (see `doc/roms-installation.md`). ByteBox handles
  their absence itself: it opens normally and routes to an in-app installer
  (F6 → "ROMs") rather than failing. Nothing for the PKGBUILD to do here.
- `~/.config/bytebox/` and `~/.bytebox/{ROM,DSK}/` are created by ByteBox
  itself on first use — nothing for `package()` to pre-create either.

## Step 4 — Generate `.SRCINFO`

AUR repositories don't store the `PKGBUILD`'s parsed metadata separately —
`.SRCINFO` is that metadata, generated from the `PKGBUILD` and required on
every push:

```sh
makepkg --printsrcinfo > .SRCINFO
```

Regenerate it every time `PKGBUILD` changes (`pkgver`/`pkgrel` bump,
dependency change...) — a stale `.SRCINFO` is a common rejection reason.

Worth doing before publishing, too: `makepkg -si` builds and installs
locally, so you can confirm the package actually works before anyone else
sees it.

## Step 5 — Create the AUR repository

Requires an AUR account with an SSH key registered first. `bytebox` and
`bytebox-git` are two separate AUR packages, each with its own repository —
repeat this step (and Step 6) once per package being published.

```sh
git clone ssh://aur@aur.archlinux.org/bytebox.git
cd bytebox
# or, for the rolling package:
# git clone ssh://aur@aur.archlinux.org/bytebox-git.git
# cd bytebox-git
```

Cloning a not-yet-existing package name succeeds and just gives an empty
repository — that's expected, not an error. Copy the matching `PKGBUILD`
(`packaging/PKGBUILD` or `packaging/PKGBUILD-git`) and its `.SRCINFO` into
it.

## Step 6 — Commit and push

```sh
git add PKGBUILD .SRCINFO
git commit -m "Initial commit of bytebox 2.0.0"
git push
```

(`"Initial commit of bytebox-git"` for the rolling package's repository.)
The first push creates the package page on the AUR. Nothing else to
configure.
