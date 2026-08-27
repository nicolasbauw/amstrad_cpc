; Minimal example built on top of mouse-driver.asm: moves character 0xF4
; of the CPC charset (a filled triangle, used here as a crude
; pseudo-pointer) around the screen, following the sign of each mouse
; delta. See mouse-interface.md.
;
; Deliberately avoids every firmware (ROM) call, like dune-cpc's own
; poc/test_souris.asm and poc/demo_curseur.asm: a snapshot built by RASM's
; BUILDSNA hands control straight to this program's own entry point,
; without ever running the ROM's own boot sequence — the RAM-resident
; firmware jumpblock it depends on (TXT_*, that sequence populates) is
; therefore never set up, and calling it does nothing (verified: a first
; version of this demo using TXT_WR_CHAR/TXT_SET_CURSOR just produced a
; blank screen). Plotting the glyph directly into screen memory sidesteps
; the whole issue, and needs no firmware at all.
;
; Vertical resolution is one PIXEL, horizontal is one character CELL (8
; pixels) — deliberately asymmetric. A first version moved by one whole
; character row (8 pixels) vertically too: harmless on paper (same step
; size as horizontally), but nowhere near as convincing in practice —
; jumping a full text-line height at once reads as imprecise/laggy in a
; way an equivalent horizontal jump doesn't, precisely because a real
; pointer's vertical motion is the axis people track most closely for
; "does this feel right". Horizontal stayed cell-based: 8px steps read
; fine on that axis, and it keeps the column math (and this file) simpler.
;
; Build + run:
;   rasm mouse-cursor-demo.asm -oi mouse-cursor-demo.sna -v2 && bb --snapshot=mouse-cursor-demo.sna
;
; Expected behaviour: a white triangle sits near the middle of a black
; screen (MODE 2, set by this program itself). With the mouse enabled (F6
; "Enable mouse") and captured (left-click in the window), moving the
; mouse steps it in the matching direction, clamped well inside the
; visible screen.

BUILDSNA
; BANK 1, not BANK 0 — see the detailed comment in dune-cpc's
; poc/test_souris.asm: RASM maps bank N to the Nth 64 KB quarter of the
; snapshot regardless of the chosen ORG, so BANK 0 + ORG #4000 would place
; the code at real address &0000 while PC pointed at &4000 (nothing
; there).
BANK 1
ORG  #4000
RUN  #4000

    jp   start

INCLUDE "mouse-driver.asm"

; MODE 2 is 640x200 pixels, 1 byte per scanline per 8-pixel-wide character
; column (1 bit per pixel) — the simplest of the three screen modes to
; plot into directly, and a convenient match for the CPC font ROM's own
; 1-bit-per-pixel glyph format (see POINTER_GLYPH below).
;
; cursor_col is a character-cell column (0-79). cursor_row is an ABSOLUTE
; PIXEL row (0-199), not a character row — see the header comment for why.
; Margins keep the 8x8 pointer well inside the visible screen at all
; times (ROW_MAX leaves room for its 8-pixel height).
COL_MIN EQU 4
COL_MAX EQU 75
ROW_MIN EQU 8
ROW_MAX EQU 184

SCREEN_BASE EQU #C000

cursor_col: defb (COL_MIN + COL_MAX) / 2
cursor_row: defb (ROW_MIN + ROW_MAX) / 2

; See main_loop's comment: BC doesn't survive a call to erase_glyph.
saved_dx: defb 0
saved_dy: defb 0

; Scanline offset (0-7) of the glyph row currently being plotted by
; draw_glyph/erase_glyph — see their comments for why this lives in
; memory rather than a register.
plot_i: defb 0

; Character 0xF4 of the CPC ROM font (core/bin/OS6128-AZERTY.rom, font
; table at offset #3900 + (code-32)*8) — a solid triangle pointing up,
; used here as a stand-in for a mouse pointer. One byte per scanline row,
; MSB = leftmost pixel, bit set = pixel on.
pointer_glyph: defb #00, #00, #18, #3C, #7E, #FF, #FF, #00

start:
    di
    ; See mouse-driver.asm's header comment: SP must point at plain RAM,
    ; never at &0000-&3FFF or &C000-&FFFF while a ROM is paged there.
    ; &8000 is plain RAM regardless of ROM configuration, well above this
    ; program and well below the &C000 screen this demo pokes directly.
    ld   sp, #8000

    ; Gate Array, "select screen mode / ROM configuration" (top two bits
    ; of the byte = %10): MODE 2 (bits 1-0 = %10), both ROMs left
    ; disabled (bits 3-2 = %11, already their state coming out of
    ; BUILDSNA — this program never needs either), interrupt bit left
    ; clear. #8E = %10001110.
    ld   bc, #7F8E
    out  (c), c

    ; Gate Array, "select pen" (top two bits = %00) then "select colour"
    ; (top two bits = %01, low 5 bits = hardware colour number) — two
    ; writes per pen. PEN 0 (paper, i.e. every unset screen bit) = hardware
    ; colour #54 (black). PEN 1 (ink, i.e. every set bit — the glyph) =
    ; hardware colour #40 (white), for maximum contrast against PEN 0.
    ld   bc, #7F00
    out  (c), c
    ld   bc, #7F54
    out  (c), c
    ld   bc, #7F01
    out  (c), c
    ld   bc, #7F40
    out  (c), c

    ; Clears the whole 16 KB MODE 2 screen (&C000-&FFFF) to 0 — every
    ; pixel PEN 0 (black). Standard LDIR fill idiom: write one zero byte,
    ; then have LDIR copy it forward &3FFF times, each copy's source
    ; being the zero byte the previous copy just wrote.
    ld   hl, SCREEN_BASE
    ld   (hl), 0
    ld   de, SCREEN_BASE + 1
    ld   bc, #3FFF
    ldir

    call draw_glyph

main_loop:
    ; Direct low-level reads (mouse_read_dx/dy), not mouse_update: this
    ; demo only cares about the sign of each poll's delta, not an
    ; unbounded accumulated position — see mouse-interface.md.
    ;
    ; Stashed in memory, not kept in BC across the erase_glyph call below:
    ; calc_pixel_addr uses BC as scratch space for its own row*80
    ; computation, and both draw_glyph/erase_glyph use B as their own loop
    ; counter — either alone silently destroys whatever the caller had in
    ; BC. Cost this test: the pointer never moved at all, dx/dy always
    ; read back as garbage by the time col_step/row_step used them.
    call mouse_read_dx
    ld   (saved_dx), a
    call mouse_read_dy
    ld   (saved_dy), a

    ld   a, (saved_dx)
    ld   b, a
    ld   a, (saved_dy)
    or   b                    ; both deltas zero -> nothing moved this poll
    jr   z, main_loop

    call erase_glyph

    ; Step the column by one cell in the sign of dx, clamped.
    ld   a, (saved_dx)
    or   a
    jr   z, row_step
    bit  7, a
    jr   nz, col_dec
    ld   a, (cursor_col)
    cp   COL_MAX
    jr   z, row_step
    inc  a
    ld   (cursor_col), a
    jr   row_step
col_dec:
    ld   a, (cursor_col)
    cp   COL_MIN
    jr   z, row_step
    dec  a
    ld   (cursor_col), a

row_step:
    ; Same logic for the row, from dy — but one pixel at a time, not one
    ; character row, see the header comment.
    ld   a, (saved_dy)
    or   a
    jr   z, redraw
    bit  7, a
    jr   nz, row_dec
    ld   a, (cursor_row)
    cp   ROW_MAX
    jr   z, redraw
    inc  a
    ld   (cursor_row), a
    jr   redraw
row_dec:
    ld   a, (cursor_row)
    cp   ROW_MIN
    jr   z, redraw
    dec  a
    ld   (cursor_row), a

redraw:
    call draw_glyph
    jr   main_loop

; --- Screen plotting -------------------------------------------------

; HL = the screen address of absolute pixel row A (0-199) at the current
; cursor_col. Splits A into a character row (A/8) and a scanline-within-
; row (A AND 7): address = SCREEN_BASE + (A/8)*80 + cursor_col +
; (A AND 7)*&800 — see mouse-interface.md for the formula and its source.
; Needed (rather than the simpler "fixed character row, +&800 per
; scanline" version this demo started with) because cursor_row is now an
; unaligned pixel offset: the 8 scanlines of one glyph can straddle two
; different character rows once vertical movement isn't restricted to
; multiples of 8. Clobbers: A, BC, DE, HL.
calc_pixel_addr:
    ld   b, a                  ; b = y, preserved for the AND 7 part below
    srl  a
    srl  a
    srl  a                     ; a = y/8 (character row, 0-24)
    ld   l, a
    ld   h, 0
    add  hl, hl                ; *2
    add  hl, hl                ; *4
    add  hl, hl                ; *8
    add  hl, hl                ; *16
    ld   d, h
    ld   e, l                  ; de = (y/8)*16
    add  hl, hl                ; *32
    add  hl, hl                ; *64
    add  hl, de                ; *64 + *16 = *80

    ld   a, b
    and  7                     ; a = y AND 7 (scanline within the row, 0-7)
    ld   c, a
    or   a
    jr   z, pixel_addr_add_col
pixel_addr_scanline_loop:
    ld   de, #0800
    add  hl, de
    dec  c
    jr   nz, pixel_addr_scanline_loop

pixel_addr_add_col:
    ld   a, (cursor_col)
    ld   e, a
    ld   d, 0
    add  hl, de
    ld   de, SCREEN_BASE
    add  hl, de
    ret

; Plots pointer_glyph at (cursor_col, cursor_row). `plot_i` (0-7) and IX
; (glyph byte pointer) carry the loop state across each calc_pixel_addr
; call instead of a register: calc_pixel_addr clobbers A/BC/DE/HL, so
; nothing there would survive anyway. Clobbers: A, BC, DE, HL.
draw_glyph:
    xor  a
    ld   (plot_i), a
    ld   ix, pointer_glyph
draw_loop:
    ld   a, (cursor_row)
    ld   b, a
    ld   a, (plot_i)
    add  a, b                  ; a = cursor_row + plot_i, this scanline's absolute pixel row
    call calc_pixel_addr
    ld   a, (ix+0)
    ld   (hl), a
    inc  ix
    ld   a, (plot_i)
    inc  a
    ld   (plot_i), a
    cp   8
    jr   nz, draw_loop
    ret

; Blanks whatever is at (cursor_col, cursor_row) — call before moving to
; the new position, so the pointer doesn't leave a trail. Clobbers: A,
; BC, DE, HL.
erase_glyph:
    xor  a
    ld   (plot_i), a
erase_loop:
    ld   a, (cursor_row)
    ld   b, a
    ld   a, (plot_i)
    add  a, b
    call calc_pixel_addr
    ld   (hl), 0
    ld   a, (plot_i)
    inc  a
    ld   (plot_i), a
    cp   8
    jr   nz, erase_loop
    ret
