# The `.SNA` snapshot format, as read and written by ByteBox

The console command `snap f.sna` (see `src/snapshot.rs`) writes the current
machine state to disk in the `.SNA` format, the de facto exchange format
between CPC emulators (originated by WinAPE, also produced and read by
Caprice32 and others). This lets a state captured in ByteBox be reloaded in
another emulator — handy on its own, but mostly useful as a diagnostic
tool: transplant an exact state elsewhere and compare behaviour from the
same starting point, without depending on a reproducible key sequence.

`snapload f.sna`, or `--snapshot=<file>` on the command line, does the
reverse. Reading was deliberately left out at first — a half-implemented
reader would be a trap, silently accepting files this emulator cannot
actually resume. It was added for a use case that justified doing it
properly: [RASM](https://github.com/EdouardBERGE/rasm) assembles Z80 source
straight into a ready-to-run `.SNA`, so `rasm demo.asm -sna demo.sna &&
bytebox --snapshot=demo.sna` replaces building a disk image between two
attempts.

## Header layout (256 bytes)

The header layout follows Caprice32's `t_SNA_header` byte for byte (packed
structure), since that is the emulator most likely to read files produced
here. All multi-byte fields are little-endian. Offsets are named in
`src/snapshot.rs`, module `off`, rather than counted by hand at each call
site.

| Offset | Size | Field | Source |
|---|---|---|---|
| 0x00 | 8 | Signature `"MV - SNA"` | fixed |
| 0x10 | 1 | Snapshot version | `1` (see below) |
| 0x11 | 2 | AF | `F` byte, then `A` |
| 0x13 | 2 | BC | `C`, then `B` |
| 0x15 | 2 | DE | `E`, then `D` |
| 0x17 | 2 | HL | `L`, then `H` |
| 0x19 | 1 | R | refresh register |
| 0x1A | 1 | I | interrupt vector register |
| 0x1B | 1 | IFF0 | `cpu.iff1()` |
| 0x1C | 1 | IFF1 | `cpu.iff2()` |
| 0x1D | 2 | IX | `IXL`, then `IXH` |
| 0x1F | 2 | IY | `IYL`, then `IYH` |
| 0x21 | 2 | SP | |
| 0x23 | 2 | PC | |
| 0x25 | 1 | Interrupt mode | 0, 1 or 2 |
| 0x26 | 2 | AF' | shadow register set |
| 0x28 | 2 | BC' | |
| 0x2A | 2 | DE' | |
| 0x2C | 2 | HL' | |
| 0x2E | 1 | Gate Array: selected pen | 0-15, 16 = border |
| 0x2F | 17 | Gate Array: palette | 16 inks + border, hardware values 0-31 |
| 0x40 | 1 | Gate Array: ROM configuration | see below, bits inverted |
| 0x41 | 1 | Gate Array: RAM configuration | `ram_config`, as written to the MMU |
| 0x42 | 1 | CRTC: selected register | 0-17 |
| 0x43 | 18 | CRTC: the 18 registers | |
| 0x55 | 1 | Currently selected upper ROM | |
| 0x56 | 1 | PPI port A | |
| 0x57 | 1 | PPI port B | |
| 0x58 | 1 | PPI port C | |
| 0x59 | 1 | PPI control register | |
| 0x5A | 1 | PSG: selected register | 0-15 |
| 0x5B | 16 | PSG: the 16 registers | |
| 0x6B | 2 | RAM size, in KB | always `128` (see below) |

Bytes not covered by any field above are left at zero.

### IFF0 / IFF1 naming

The format's own field names are `IFF0`/`IFF1`, carried over here as-is even
though it is easy to misread against our two-flip-flop model: `IFF0` holds
what this emulator calls `iff1()`, and `IFF1` holds `iff2()`. This is a
naming quirk of the `.SNA` format itself, not a bug — flagged here because
it looks exactly like an off-by-one at first glance.

### ROM configuration bits are inverted

`0x40` packs the video mode in bits 0-1, then bit 2 for the low ROM and bit
3 for the upper ROM — set to **1 when the ROM is disabled**, the opposite of
this emulator's own `rom_low_enabled` / `rom_high_enabled` flags. Getting
this backwards produces a snapshot that boots into the wrong memory
configuration on reload; covered by
`the_rom_configuration_bits_are_inverted_relative_to_our_flags` in
`snapshot.rs`.

### Snapshot version

The header declares version 1: registers, Gate Array, CRTC, PPI, PSG and
RAM. Versions 2 and 3 of the format add fields for things this emulator
does not model identically (machine model byte, FDC state, internal CRTC
counters...); declaring a higher version would promise data this writer
does not actually produce.

## RAM dump

Immediately after the 256-byte header: **128 KB of RAM**, uncompressed, in
CPC physical bank order (banks 0-7, 16 KB each) — regardless of the current
paging configuration. A saved file is therefore always exactly
`256 + 131072 = 131328` bytes, checked by
`a_saved_file_has_the_expected_size` in `snapshot.rs`.

Any RAM beyond the standard 128 KB (see `config.toml`, `[memory]
extra_ram_banks` — third-party 6128 expansions, Dk'tronics-style) is **not**
written: the `.SNA` format has no representation for it, and including it
would produce a file no other emulator could read back correctly. `save()`
only ever reads the first 128 KB of the underlying RAM buffer.

## How loading restores the machine

`load` mirrors Caprice32's own loader on two points that matter:

1. **Power-cycle first.** The machine is reset before anything is restored,
   so loading starts from a known state — RAM zeroed, devices at their
   defaults, ROMs reloaded — instead of layering the snapshot over whatever
   was running, where any field the format doesn't carry would silently
   keep its previous value.
2. **Replay I/O writes rather than poke fields.** The Gate Array, CRTC,
   upper ROM, PPI and PSG are restored by writing to their ports, exactly
   as the original program did. Everything derived from those writes — the
   memory's ROM-enable flags, the selected keyboard line, the tape motor,
   PSG register masks, the envelope restart on R13 — then falls into place
   on its own.

One deliberate divergence from Caprice32: the **PPI control register is
written first**, not last. On a real 8255 (and in `Ppi::write_register`,
where the behaviour is required by Barbarian) configuring it clears ports A
and C, so writing it after the ports would wipe what was just restored.

The interrupt state (`IM`, `IFF1`, `IFF2`) is restored through
`CPU::set_interrupt_state`, added to the `zilog_z80` crate for this: those
three values are otherwise only ever changed by `IM n`, `EI` and `DI`, so a
host restoring a state had no way to put them back.

## Deliberately out of scope

- **Version 3 snapshots with chunked memory.** When the RAM size at `0x6B`
  is zero, the memory lives in `MEM0`-`MEM8` chunks after the header,
  possibly compressed. `load` refuses these outright rather than loading a
  machine whose memory would be entirely blank — which would look like an
  emulator crash rather than an unsupported format. Caprice32 rejects them
  too. Chunks *following* a normal flat dump are simply skipped, as the
  format intends.
- **Version 2/3 header fields.** Model, FDC state, internal CRTC counters,
  PSG envelope step and so on are read past, not applied — except the CPC
  model, which only produces a warning when it isn't a 6128 (a 464 snapshot
  loads, but its firmware expectations don't match this machine).
- **Extended RAM beyond 128 KB.** No representation in the format; silently
  truncated rather than attempted.
- **FDC state, tape state, snapshot versions 2/3 fields.** Would require
  either extending the format in a way other emulators cannot read, or
  describing state we do not model closely enough to claim compatibility.
