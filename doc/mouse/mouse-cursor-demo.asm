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
; Both axes move at ONE PIXEL per poll. An earlier version stepped the
; column by one whole character cell (8 pixels) while the row already
; moved by one pixel: harmless in isolation (each axis felt fine tested on
; its own), but obvious and disorienting on a diagonal move, where the
; same physical mouse motion produced 8x more travel horizontally than
; vertically. Pixel-precise columns cost real complexity: a glyph no
; longer starts on an 8-pixel (one screen byte) boundary, so its 8x8
; pattern must be split, per scanline, across the two screen bytes it now
; straddles (see draw_glyph) — MODE 2 packs 8 pixels per byte, there's no
; way around it for a byte-addressed screen.
;
; Build + run:
;   rasm mouse-cursor-demo.asm -oi mouse-cursor-demo.sna -v2 && bb --snapshot=mouse-cursor-demo.sna
;
; Expected behaviour: a white triangle sits near the middle of a black
; screen (MODE 2, set by this program itself). With the mouse enabled (F6
; "Enable mouse") and captured (left-click in the window), moving the
; mouse steps it in the matching direction, one pixel at a time on both
; axes, clamped well inside the visible screen.

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

; MODE 2 is 640x200 pixels, 1 bit per pixel, 8 pixels (1 byte) per screen
; byte — the simplest of the three screen modes to plot into directly,
; and a convenient match for the CPC font ROM's own 1-bit-per-pixel glyph
; format (see POINTER_GLYPH below).
;
; cursor_col and cursor_row are both ABSOLUTE PIXEL coordinates (0-639,
; 0-199) — see the header comment for why the column needs to be, unlike
; an earlier version of this file. cursor_col is 16-bit (defw): 600 alone
; doesn't fit in a byte. Margins keep the 8x8 pointer well inside the
; visible screen at all times (the MAX constants leave room for its
; 8-pixel width/height).
COL_MIN EQU 8
COL_MAX EQU 600
ROW_MIN EQU 8
ROW_MAX EQU 184

SCREEN_BASE EQU #C000

cursor_col: defw (COL_MIN + COL_MAX) / 2
cursor_row: defb (ROW_MIN + ROW_MAX) / 2

; See main_loop's comment: BC doesn't survive a call to erase_glyph.
saved_dx: defb 0
saved_dy: defb 0

; Scanline offset (0-7) of the glyph row currently being plotted by
; draw_glyph/erase_glyph — see their comments for why this lives in
; memory rather than a register.
plot_i: defb 0

; cursor_col decomposed into a screen byte column (cursor_col/8) and a
; bit offset within it (cursor_col AND 7), computed once per draw_glyph/
; erase_glyph call (it doesn't change between the 8 scanlines of a single
; call) rather than once per scanline.
glyph_byte_col: defw 0
glyph_shift: defb 0

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
    ; calc_row_addr uses BC as scratch space for its own row*80
    ; computation, and both draw_glyph/erase_glyph use B/C internally too
    ; — any of them alone would silently destroy whatever the caller had
    ; in BC. Cost this the first time round: the pointer never moved at
    ; all, dx/dy always read back as garbage by the time col_step/
    ; row_step used them.
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

    ; Step the column by one pixel in the sign of dx, clamped. 16-bit
    ; compare against COL_MIN/COL_MAX: Z80 has no direct 16-bit CP,
    ; "sbc hl,de" against a disposable copy of cursor_col stands in for
    ; one (it clobbers HL, hence reloading it below on the branch not
    ; taken).
    ld   a, (saved_dx)
    or   a
    jr   z, row_step
    bit  7, a
    jr   nz, col_dec
    ld   hl, (cursor_col)
    ld   de, COL_MAX
    or   a
    sbc  hl, de
    jr   z, row_step           ; already at COL_MAX
    ld   hl, (cursor_col)
    inc  hl
    ld   (cursor_col), hl
    jr   row_step
col_dec:
    ld   hl, (cursor_col)
    ld   de, COL_MIN
    or   a
    sbc  hl, de
    jr   z, row_step           ; already at COL_MIN
    ld   hl, (cursor_col)
    dec  hl
    ld   (cursor_col), hl

row_step:
    ; Same logic for the row, from dy — 8-bit here, cursor_row's range
    ; (0-199) fits a byte, so a plain CP does the job.
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

; HL = SCREEN_BASE + (A/8)*80 + (A AND 7)*&800 for absolute pixel row A
; (0-199) — the row component of the address formula, EXCLUDING the
; column (callers add glyph_byte_col themselves once, not per scanline —
; see draw_glyph/erase_glyph). Splitting A into a character row (A/8) and
; a scanline-within-row (A AND 7) is what lets a glyph's 8 scanlines
; straddle two different character rows, needed as soon as vertical
; movement isn't restricted to multiples of 8 — see mouse-interface.md for
; the full formula and its source. Clobbers: A, BC, DE, HL.
calc_row_addr:
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
    jr   z, row_addr_done
row_addr_scanline_loop:
    ld   de, #0800
    add  hl, de
    dec  c
    jr   nz, row_addr_scanline_loop
row_addr_done:
    ld   de, SCREEN_BASE
    add  hl, de
    ret

; Splits byte A into the two screen bytes an 8-pixel-wide glyph row
; produces once shifted right by C (0-7) pixels: H = the left byte's
; contribution (A's high bits), L = the right byte's (A's low bits,
; pushed up to the top) — standard trick, an 8-bit shift of A:0 treated as
; one 16-bit value, shifted right C times. C=0 (no split needed) is
; handled directly (H=A, L=0) rather than looping zero times, if only for
; clarity. DE is left untouched on purpose: draw_glyph stashes the target
; screen address there across this call. Clobbers: A, B, HL.
shift_glyph_byte:
    ld   h, a
    ld   l, 0
    ld   b, c
    ld   a, b
    or   a
    ret  z
shift_glyph_byte_loop:
    srl  h
    rr   l
    djnz shift_glyph_byte_loop
    ret

; Plots pointer_glyph at (cursor_col, cursor_row). glyph_byte_col/
; glyph_shift (cursor_col decomposed, see their doc), plot_i (0-7) and IX
; (glyph byte pointer) carry state across each calc_row_addr/
; shift_glyph_byte call instead of registers: both clobber A/BC/HL (and
; shift_glyph_byte takes over HL entirely for its own computation), so
; nothing there would survive anyway — the in-progress screen address is
; kept in DE instead, the one register neither call touches. Clobbers: A,
; BC, DE, HL, IX.
draw_glyph:
    ld   hl, (cursor_col)
    ld   a, l
    and  7
    ld   (glyph_shift), a
    srl  h
    rr   l
    srl  h
    rr   l
    srl  h
    rr   l
    ld   (glyph_byte_col), hl

    xor  a
    ld   (plot_i), a
    ld   ix, pointer_glyph
draw_loop:
    ld   a, (cursor_row)
    ld   b, a
    ld   a, (plot_i)
    add  a, b                  ; a = cursor_row + plot_i, this scanline's absolute pixel row
    call calc_row_addr
    ld   de, (glyph_byte_col)
    add  hl, de                ; hl = address of the left of the two bytes this scanline touches
    ex   de, hl                ; de = that address; shift_glyph_byte needs hl for itself

    ld   a, (glyph_shift)
    ld   c, a
    ld   a, (ix+0)              ; this scanline's source byte
    call shift_glyph_byte       ; h = left byte's contribution, l = right byte's
    ld   a, h
    ld   (de), a
    inc  de
    ld   a, l
    ld   (de), a

    inc  ix
    ld   a, (plot_i)
    inc  a
    ld   (plot_i), a
    cp   8
    jr   nz, draw_loop
    ret

; Blanks whatever is at (cursor_col, cursor_row) — call before moving to
; the new position, so the pointer doesn't leave a trail. Always clears
; both bytes a glyph could touch, regardless of glyph_shift: harmless even
; when the glyph happened to be byte-aligned (the second byte is then
; already 0, nothing else is ever drawn on this screen) and much simpler
; than special-casing it. Clobbers: A, BC, DE, HL.
erase_glyph:
    ld   hl, (cursor_col)
    srl  h
    rr   l
    srl  h
    rr   l
    srl  h
    rr   l
    ld   (glyph_byte_col), hl

    xor  a
    ld   (plot_i), a
erase_loop:
    ld   a, (cursor_row)
    ld   b, a
    ld   a, (plot_i)
    add  a, b
    call calc_row_addr
    ld   de, (glyph_byte_col)
    add  hl, de
    ld   (hl), 0
    inc  hl
    ld   (hl), 0
    ld   a, (plot_i)
    inc  a
    ld   (plot_i), a
    cp   8
    jr   nz, erase_loop
    ret
