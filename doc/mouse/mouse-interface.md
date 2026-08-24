# Mouse interface — ByteBox software mouse

## Context and decision

The mouse ByteBox emulates is **not** a reproduction of a real hardware
adapter (AMX Mouse, SYMBiFACE II/X-MEM...). Decision made while developing
the [dune-cpc](https://github.com/nicolasbauw) project (the first consumer
of this interface): the "match an existing standard" route was dropped
after research — reliable documentation of these protocols has vanished
(CPCWiki pages gone), and the public who actually owns this hardware is
essentially nonexistent. Since ByteBox and the software that uses it are
developed by the same team, there's nothing to gain compatibility-wise by
chasing an unreliable external protocol.

The chosen protocol still takes inspiration from classic relative mice
(X/Y deltas + button state), as a general design choice — not to imitate
an existing standard. It keeps the door open, at little cost, should real
hardware support ever become relevant, without that being a current goal.
Real CPC hardware obviously has no mouse support at all: software that
depends on one needs a joystick/keyboard fallback.

## Protocol

Three ports, on the `&FC00-&FCFF` page reserved for ByteBox software
devices with no hardware equivalent (`core/src/bus.rs`,
`bytebox_mouse_selected` — doesn't overlap the decoding of any real
component):

| Port | Register | Behaviour |
|---|---|---|
| `&FC00` | Delta X | Signed byte. **Consumed on read** (reset to 0 on the core side). |
| `&FC01` | Delta Y | Same as `&FC00`. |
| `&FC02` | Buttons | Bit 0 = left, bit 1 = right. **Not** consumed (repeated reads with no change return the same value). |

The middle button never reaches the CPC — intercepted on the ByteBox side
to capture/release the mouse (see below) — so it has no dedicated bit on
`&FC02` in practice, even though `core::mouse::Mouse` reserves bit 2 for
it internally.

Disabled by default. Port reads as undecoded (`0xFF`) while disabled, so a
driver can't confuse "absent" with "not moving". Three ways to enable it:
- `config.toml`, `[mouse]` section, `enabled = true`;
- the `mouse on`/`mouse off` hot console command (F10/F11);
- the "Enable mouse" checkbox in the configuration panel (F6).

## Capture (`bytebox/src/sdl.rs`)

Left-click in the main window (outside the F6/F7/F10 overlays, mouse
enabled in the configuration) captures the host mouse in SDL2 relative
mode; that click isn't forwarded to the CPC. Middle-click releases —
likewise absorbed, never forwarded. Released automatically on focus loss,
opening an overlay, or being disabled mid-session. Cursor hiding on hover
over the window (existing behaviour) is unaffected.

## Low-level RASM driver (`mouse-driver.asm`)

Provided in this same directory, ready to assemble/include as-is in a
RASM project. Register calling convention (CALL/RET/PUSH/POP), not a C
convention (`__sdcccall` or otherwise) — wrap it separately if the rest of
the calling project is written in C.

- `mouse_update`: reads all three ports, accumulates the deltas into
  `mouse_x`/`mouse_y` (16-bit signed, **unclamped** — the frame of
  reference, screen or map, is up to the caller). Updates
  `mouse_buttons`/`mouse_previous_buttons`. Call once per game loop
  iteration.
- `mouse_left_click_evt` / `mouse_right_click_evt`: rising-edge detection
  (call after `mouse_update`), for an "event" click rather than the raw
  state — useful for a menu/button, for instance.
- `mouse_read_dx` / `mouse_read_dy` / `mouse_read_buttons`: direct
  low-level access to the three ports, if `mouse_update` doesn't fit.

Pitfall encountered and documented in the driver's own comments:
`CALL`/`RET` require SP to point at plain RAM, never at a potentially
paged ROM area (upper `&C000-&FFFF` or lower `&0000-&3FFF`) — the ROM only
masks **reads** at these addresses (not writes), which silently corrupts
any return address pushed there. A RASM snapshot's default stack
(`SP=&C000`) lands right in the middle of it.

Validated by manual testing under real conditions (mouse captured,
click/motion, visual feedback on screen) in the dune-cpc project.

## Example: moving a pointer (`mouse-cursor-demo.asm`)

A complete, self-contained program built on top of `mouse-driver.asm`:
moves a small triangle (character `0xF4` of the CPC's ROM font) around
the screen, one cell at a time, in the direction of each mouse delta.

```
rasm mouse-cursor-demo.asm -oi mouse-cursor-demo.sna -v2 && bb --snapshot=mouse-cursor-demo.sna
```

**No firmware calls** (no `TXT_*`), deliberately, like dune-cpc's own
POCs: a snapshot built by RASM's `BUILDSNA` hands control straight to the
program, without ever running the real ROM boot sequence — the RAM-based
firmware jumpblock that sequence normally populates is therefore left
empty, and calling into it does nothing (verified: an earlier version of
this demo using `TXT_WR_CHAR`/`TXT_SET_CURSOR` just produced a blank
screen). The demo instead pokes bytes directly into screen memory (MODE 2,
which it configures itself — 1 bit per pixel, exactly matching the ROM
font's own format), at address `&C000 + row*80 + column + scanline*&800`
(25 character rows, 8 scanlines each, `&800` bytes between two consecutive
scanlines of the same row).

Pitfall hit and fixed while building this: `calc_cursor_addr`/
`draw_glyph`/`erase_glyph` all use `BC` internally (scratch computation or
loop counter) — a caller still holding the mouse delta there loses it
silently on the next call. Deltas are therefore stashed in memory, not
kept in a register, across these calls.
