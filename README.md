# ByteBox - an Amstrad CPC 6128 Emulator

## Installation

None of these packages bundle the CPC's system ROMs (legal status
unclear — see "Missing ROMs" below); the emulator still opens fine
without them and walks you through installing them on first launch.

**Arch Linux (AUR) — coming soon:** the AUR is currently not accepting new
maintainer accounts, following recent attacks — these packages aren't
published there yet.

```sh
paru -S bytebox
# or, to track the latest commit instead of a tagged release:
paru -S bytebox-git
```

(No `paru`? Any AUR helper works the same way, or build manually — see
`doc/create-pkgbuild-aur.md`.)

**macOS (Homebrew):**

```sh
brew tap nicolasbauw/bytebox
brew install bytebox
```

Recent Homebrew versions ask you to explicitly trust a third-party tap the
first time (it'll print the exact command) — expected, not an error.

This builds from source (`rust`, pulled in automatically, is the only
non-trivial dependency) and also installs `ByteBox.app`; see the
"Caveats" note `brew` prints at the end for the one extra command to add
it to `/Applications`.

**Linux (AppImage), any distribution:**

Download the latest `bytebox-*-x86_64.AppImage` from the
[Releases page](https://github.com/nicolasbauw/amstrad_cpc/releases), then:

```sh
chmod +x bytebox-*-x86_64.AppImage
./bytebox-*-x86_64.AppImage
```

**Windows:**

Download the latest `bytebox-*-x86_64.msi` from the
[Releases page](https://github.com/nicolasbauw/amstrad_cpc/releases) and
run it.

**Build from source (any platform):**

```sh
git clone https://github.com/nicolasbauw/amstrad_cpc.git
cd amstrad_cpc
cargo build --release
./target/release/bytebox
```

## Command-line options

- `--diag` : additional diagnostics ROM (0F slot)
- `--disk=<file>` / `-d <file>`: load a disk image on drive A at startup. A bare
  filename (no path) is looked up in `[file] dsk_path` from `config.toml` if it
  isn't found as given.
- `--autocmd=<command>` / `-a <command>`: type a command at the emulated keyboard
  once BASIC is ready, exactly like Caprice32's own `--autocmd`. Handy for
  jumping straight into a game during a debugging session:

  ```sh
  bytebox --disk=Barbarian.dsk --autocmd='RUN"BARBA.I'
  ```
- `--snapshot=<file>` / `-s <file>`: resume from a `.SNA` snapshot instead of
  booting. A bare filename is looked up in `~/.bytebox/SNA` (or `[file]
  sna_path` from `config.toml`), the same directory the `snap` console
  command writes to. Mostly for Z80 development:
  [RASM](https://github.com/EdouardBERGE/rasm) can assemble straight to a
  ready-to-run snapshot, so there's no disk image to build between two
  attempts:

  ```sh
  rasm demo.asm -sna demo.sna && bytebox --snapshot=demo.sna
  ```

## Function keys

    F1 / F2 / F3   window size x1 / x2 / x3
    F4             toggle fullscreen
    F5             toggle the CRT shader (RGB phosphor mask, scanlines —
                   see below)
    F6             toggle the configuration/media panel (disks, tape,
                   drive B, extra RAM, zoom, volume, CRT shader tuning,
                   ROM installation — all with an immediate effect, no
                   restart needed)
    F7             toggle the virtual keyboard (clickable, overlaid on the
                   emulator window)
    F8             step to next Z80 instruction
    F9             step to next video line
    F10            toggle the quick command bar (one input line, overlaid
                   on the emulator window)
    F11            toggle the full console window (scrollable history,
                   same commands as the quick command bar) — on macOS,
                   Cmd+Shift+C does the same thing, since F11 is claimed
                   system-wide by "Show Desktop" before it ever reaches
                   the app
    F12            toggle the machine status window

The emulator no longer depends on the terminal it was launched from: every
command and every message goes through the quick command bar (`F10`) or the
full console window (`F11`), so launching from a desktop icon works exactly
like launching from a terminal.

This function key table, along with the "Emulator commands" and "Emulator
monitor commands" sections further down, are also available in-app, without
alt-tabbing to this file: the "Help" tab of the configuration panel (`F6`).

## Missing ROMs

ByteBox doesn't ship the CPC's system ROMs (OS, BASIC, AMSDOS, diagnostic)
— their legal status has never been formally clarified by Amstrad, so
they're not bundled or downloaded silently. `Machine::load_roms` expects
them in `~/.bytebox/ROM/` by default, or wherever `[rom]` in `config.toml`
points instead if you've set it.

The "ROMs" tab of the configuration panel (`F6`) reflects that exact
lookup, `[rom]` overrides included: it checks whether the files
`load_roms` would actually use are present, not where they came from — a
ROM you already had from somewhere else works exactly as well as one
installed through this screen, and is recognized as installed the same
way. Only when they're missing does the tab show anything to do: a
disclaimer to accept (Amstrad has neither granted nor refused permission
to redistribute these ROMs; usage is widely tolerated in the
retro-computing community, but their legal status isn't settled) and an
"Install ROMs" button.

That button downloads from exactly two sources — no others, and no
`[rom]` override changes what it fetches, only whether the tab decides
it's needed in the first place: the AZERTY system+AMSDOS ROMs from
[genesis8bit.fr](https://www.genesis8bit.fr/), and the diagnostic ROM
from the [amstrad-diagnostics](https://github.com/llopis/amstrad-diagnostics)
GitHub project (details, including why an initially-considered third
source was dropped, in `doc/roms-installation.md`).

If they're missing at startup (first launch, or a package install without
a separate ROM-provisioning step), the emulator still opens — it runs on
blank ROMs, harmlessly, rather than failing before a window ever appears
— and `F6` opens automatically on this tab. Once installed, the machine
power-cycles automatically — no restart needed.

## CRT shader

`F5` reconstructs the image the way a real tube would: an RGB phosphor mask
(each screen pixel belongs to a red, green or blue column, staggered every
other row) plus a proper electron-beam profile (soft horizontal blending
between neighboring columns, a real dark gap between scanlines rather than
just a darkened pixel). The beam reconstruction is computed in source-pixel
space, so it keeps the same proportions at every zoom level; the phosphor
mask is sized in real output pixels (a property of the simulated tube, not
of the source image), so it stays visible at `x1` too, not just from `x2` up.
Scanlines, on the other hand, may be hard to perceive at `x1`: each one only
spans a couple of output pixels there. `x2` and up make them much clearer.

Scanlines follow the *real* CPC scanline period, not the frame buffer's:
`video::render` draws each CPC scanline twice (600 buffer rows = 300 real
scanlines), so one band per buffer row would draw twice as many as the
machine ever had, each half as tall.

Bright areas show weaker scanlines than dark ones, on purpose: a more
intense electron beam is physically wider, so it fills more of the gap. That
"beam bloom" is what keeps whites white without a global brightness multiply
— such a multiply clips the highlights while lifting the troughs, which
flattens the very scanline contrast it was meant to restore.

Horizontal edges are softened by an adjustable Gaussian beam spot: a real
CPC feeds the tube an analog signal of limited bandwidth, so nothing ever
ends on a hard edge *across* a line. Only across — each line is scanned
separately, which is why the vertical direction stays governed by the
scanline beam instead. At 0 the spot collapses onto the nearest texel and
you get the original sharp pixels back.

Every constant behind this effect — mask cell size, mask/scanline strength,
scanline beam width, beam bloom, brightness boost, horizontal blur — is a
slider in the "CRT Shader" tab of the configuration panel (`F6`), with
"Reset to defaults" and "Save to config.toml" buttons. It's a separate tab
from the rest of the panel (media, hardware, display, audio) because eight
sliders together no longer fit an `x1` window. The shipped defaults look
good on ordinary displays too, not just high-density ones — adjust to
taste from there.

Saving writes a `[crt]` section holding every slider's current value, and
only that section — the rest of your `config.toml`, comments included, is
left untouched, and saving twice replaces the section rather than stacking
a second one. Each field read back at startup overrides the corresponding
built-in default on its own, so a hand-written partial section is fine:

```toml
[crt]
scanline_beam = 9.0
mask_cell_px = 2.0
```

## Emulator commands
    disk d.dsk          Loads the d.dsk disk image on drive A
    disk d.dsk b        Loads the d.dsk disk image on drive B (if enabled in config.toml)
    disk eject          Ejects the disk image from drive A
    disk eject b        Ejects the disk image from drive B
    blank d.dsk         Creates a blank formatted disk image and inserts it in drive A
    blank d.dsk b       Creates a blank formatted disk image and inserts it in drive B
    tape f.cdt          Loads the f.cdt tape image into the tape reader
    tape eject          Ejects the tape image
    snap f.sna          Saves a .SNA snapshot (readable by other CPC emulators)
    pc                  Performs a power cycle
    vol                 Displays the audio output volume
    vol 30              Sets the audio output volume to 30 %
    driveb on           Enables drive B, with immediate effect (unlike
                        config.toml's [drives] drive_b, which only
                        applies at startup)
    driveb off          Disables drive B
    ram 16              Sets extra RAM banks to 16 (applied at the next
                        power cycle, "pc" — RAM is sized at construction)
    tapevol 10          Sets the tape signal level in the audio mix to 10 %
    diag on             Enables the Diagnostic ROM at slot 0F (applied at
                        the next power cycle, "pc")
    diag off            Disables it

All of the above are also available from the configuration/media panel
(**`F6`**) as clickable fields, with a native file picker for disk and tape
images — no need to type a path.

## Tape drive

The 6128 has a floppy drive, but the emulator also reproduces a real datacorder: loading `.cdt` images (see the
`tape` console command above, or `Machine::load_tape`), turning the motor on
and off from the PPI exactly as the firmware does, and even reinjecting the
tape signal into the audio mix while the motor runs, for the familiar
loading whistle. A power cycle (`pc`) keeps the tape inserted, rewound to
its start, just like switching a real CPC off and on again with a cassette
still in the deck.

At the BASIC prompt, **`CTRL` + the numeric keypad's `Enter`** (not the main
Enter key) types `RUN"` and validates it automatically — this is a keyboard
expansion token built into the firmware itself, not an emulator feature.
Once a tape is inserted, this is normally followed by "Press PLAY then any
key", after which the emulated datacorder starts feeding the firmware.

![Screenshot](assets/tape.png)
  
## Emulator monitor commands
  
    d 0x0000          disassembles code at 0x0000 and the 20 next
                      instructions
    m 0xeeee          displays memory content at address 0xeeee
    m 0xeeee 0xaa     sets memory address 0xeeee to the 0xaa value
    mr 0x1000         dumps 256 raw RAM bytes from 0x1000, ignoring any ROM
    mr 0x1000 0x1100  dumps the raw RAM range 0x1000..0x1100
    s 0xaa            searches for a byte in memory
    n                 steps to next Z80 instruction
    l                 steps to next video line
    j 0x0000          jumps to 0x0000 address
    b                 displays set breakpoints
    b 0x0002          sets a breakpoint at address 0x0002
    f 0x0002          "frees" (deletes) breakpoint at address 0x0002
    w                 displays set watchpoints
    w 0xeeee          adds a write watchpoint at address 0xeeee
    fw 0xeeee         removes watchpoint at address 0xeeee
    p                 pause execution
    g                 resume execution after the "p" command, or a breakpoint,
                      has been used to halt execution
    hw                displays Gate Array and CRTC status
    hw kb             keyboard test
    r                 displays the contents of flags, registers and interrupts
    t                 displays trace status
    t on              records every executed instruction in a ring buffer
    t calls           records only jumps, calls and returns (far longer reach)
    t off             stops recording, keeping what has been captured
    t dump 100        displays the last 100 recorded instructions
    t save f.txt      writes the whole buffer to a file
    
## Machine status window

A concrete example of combined usage:

1. launch the emulator.
2. open the machine status window alongside it using **`F12`**.
3. enter a breakpoint command in the quick command bar or the full console (e.g., `b 0x0038` for the interrupt) using **`F10`** or **`F11`**.
4. As soon as the breakpoint is hit, the emulator freezes, and the debug window displays the full machine state.
5. press **`F8`** several times: you see the disassembly print out in the console, and all register values ​​(PC, SP, A, B, C...) update in the debug window.
6. press **`F9`**: the emulator skips ahead by one video line, and you watch the `HSYNC Counter` and `current_line` update in real-time on the debugger screen.

![Screenshot](assets/machine_status.png)

## Writing Z80 code for the CPC

If you're developing *for* the CPC rather than just running software on it,
snapshots give you an edit-assemble-run loop with no disk image in the way.
[RASM](https://github.com/EdouardBERGE/rasm) assembles straight into a
ready-to-run `.SNA` — RAM laid out and entry point already set — which
ByteBox resumes directly:

Here's a complete, working example to copy-paste. Save it as `demo.asm`:

```asm
    BUILDSNA            ; emit a snapshot, not a cartridge
    BANK 0              ; without this, nothing is written to memory
    ORG  #4000
    RUN  #4000          ; where execution starts

start
    ld   bc, #7F10      ; select the border
    out  (c), c
    ld   bc, #7F4C      ; ink 12: bright red
    out  (c), c
loop
    jr   loop           ; stay here
```

Then assemble it and run it:

```sh
rasm demo.asm -oi demo.sna -v2 && bytebox --snapshot=demo.sna
```

You should get a bright red border around a blue screen. `-oi` names the
snapshot RASM writes; `-v2` picks the format ByteBox reads — see the notes
below, both matter more than they look.

The whole cycle is a single command away, so re-running after an edit costs
nothing — which is the point.

A few things worth knowing:

- **Use `-v2`.** RASM defaults to version 3 snapshots, which store memory
  in compressed `MEM0`-`MEM8` chunks (the RAM size field is left at zero).
  ByteBox refuses those outright rather than loading a machine with blank
  memory — see `doc/sna-format.md`. Version 2 writes a plain memory dump
  and loads fine. `BUILDSNA V2` in the source does the same as `-v2`.
- **Don't forget `BANK 0`.** Without it RASM assembles your code but writes
  nothing into the snapshot's memory, and says so only through a
  `Warning: No byte were written in snapshot memory` — no file is produced
  at all.
- **Where snapshots live.** A bare filename is looked up in
  `~/.bytebox/SNA` (override with `[file] sna_path` in `config.toml`), the
  same directory the `snap` console command writes to. A name containing a
  path is used as-is, so `--snapshot=./demo.sna` works from a build
  directory.
- **Snapshots go both ways.** `snap f.sna` from the console (`F10`/`F11`)
  captures the current state, `snapload f.sna` restores it. Handy to park a
  hard-to-reach state — a level, a crash, a specific interrupt moment — and
  come back to it, in ByteBox or in another emulator.
- **The debugger is right there.** Since a snapshot restores registers,
  RAM and hardware state exactly, `F12` (machine status), breakpoints and
  single-stepping (`F8`) all work from the moment it loads.

## Development builds

The window title carries a build identifier, so a build in progress is
never mistaken for a released one: the short commit hash when built from
a git checkout (`cargo build`, `bytebox-git`, CI), or the version number
from `Cargo.toml` when built from a release tarball (the AUR `bytebox`
package, Homebrew), whose archive carries no `.git` to read a hash from.
