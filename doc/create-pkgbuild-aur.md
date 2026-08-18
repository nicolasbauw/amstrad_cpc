# Creating a PKGBUILD for the AUR

Original draft generated from a web search; reviewed and rewritten here
against ByteBox's actual `Cargo.toml`, `packaging/bytebox.desktop`, and
build setup — a few things in the generic version didn't hold up (missing
`build()`/`package()` functions, no mention of the `BYTEBOX_PACKAGED_BUILD`
build flag, no checksums, generic paths instead of this project's own).

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

## Step 3 — A working PKGBUILD for ByteBox

ByteBox has no tagged release yet, so the practical way to package it today
is a **VCS package** (conventionally named `<pkgname>-git`), which always
builds from the latest commit on a given branch instead of a fixed release
tarball. Switching to a versioned `bytebox` package later (once a `v1.0.0`
tag exists) mainly means replacing the `source`/`pkgver()` below with a
release tarball URL and a real `sha256sums` — noted inline where that
would change.

```sh
# Maintainer: Your Name <you@example.com>
pkgname=bytebox-git
pkgver=r1.abcdef0  # placeholder — recomputed by pkgver() below
pkgrel=1
pkgdesc="Amstrad CPC 6128 emulator"
arch=('x86_64')
url="https://github.com/nicolasbauw/amstrad_cpc"
license=('MIT')
depends=('sdl2')
makedepends=('rust' 'git')
provides=('bytebox')
conflicts=('bytebox')
source=("$pkgname::git+$url.git#branch=master")
sha256sums=('SKIP')  # VCS source: integrity comes from git itself

pkgver() {
  cd "$pkgname"
  # Nothing tagged yet: falls back to "r<commit count>.<short hash>",
  # the standard scheme for AUR -git packages without upstream tags.
  printf "r%s.%s" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

build() {
  cd "$pkgname"
  # Marks this as an official build: without it, the window/taskbar icon
  # carries a diagonal red "dev build" stripe (see README, "Development
  # builds"). `--profile dist` (not `--release`): a slower-to-compile but
  # smaller/faster profile reserved for distributed binaries (LTO, single
  # codegen unit — see the root Cargo.toml).
  export BYTEBOX_PACKAGED_BUILD=1
  cargo build --profile dist --locked
}

package() {
  cd "$pkgname"
  install -Dm755 target/dist/bytebox "$pkgdir/usr/bin/bytebox"
  install -Dm644 packaging/bytebox.desktop \
    "$pkgdir/usr/share/applications/bytebox.desktop"
  install -Dm644 assets/bytebox_icon.png \
    "$pkgdir/usr/share/icons/hicolor/256x256/apps/bytebox.png"
  install -Dm644 LICENSE "$pkgdir/usr/share/licenses/$pkgname/LICENSE"
}
```

Notes specific to this project:

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

Requires an AUR account with an SSH key registered first.

```sh
git clone ssh://aur@aur.archlinux.org/bytebox-git.git
cd bytebox-git
```

Cloning a not-yet-existing package name succeeds and just gives an empty
repository — that's expected, not an error. Copy `PKGBUILD` and `.SRCINFO`
into it.

## Step 6 — Commit and push

```sh
git add PKGBUILD .SRCINFO
git commit -m "Initial commit of bytebox-git"
git push
```

The first push creates the package page on the AUR. Nothing else to
configure.
