; Low-level driver for the ByteBox software mouse (ports &FC00-&FC02).
; See mouse-interface.md for the protocol and the design decision behind
; it (a ByteBox-proprietary interface, not a real hardware adapter).
;
; Calling convention: plain Z80 registers — not a C ABI (SDCC
; __sdcccall(1) or otherwise). Wrap this module separately if the rest of
; your project is written in C; as-is, it assembles and works standalone
; in RASM.
;
; This driver only listens: the mouse must be enabled on the ByteBox side
; (F6 panel or config.toml [mouse]) and captured (left-click in the
; window) for these ports to return anything other than 0xFF.
;
; Every routine below uses CALL/RET/PUSH/POP: the caller must therefore
; point SP at plain RAM, NEVER at &C000-&FFFF (upper ROM) or &0000-&3FFF
; (lower ROM) while either is paged in — a POP there reads back the ROM
; instead of what a PUSH actually wrote (the CPC's ROM only masks READS
; at these addresses, not writes), silently corrupting any return address
; pushed there. Encountered in practice: a RASM snapshot's default stack
; (SP=&C000) lands right in the middle of it.

MOUSE_PORT_DX      EQU #FC00
MOUSE_PORT_DY      EQU #FC01
MOUSE_PORT_BUTTONS EQU #FC02

; Button register masks (core/src/mouse.rs, ByteBox side). The middle
; button never reaches the CPC: it's intercepted on the ByteBox side to
; release the mouse capture (see bytebox/src/sdl.rs) — no need to test for
; it here, hence no MOUSE_MASK_MIDDLE.
MOUSE_MASK_LEFT  EQU 1
MOUSE_MASK_RIGHT EQU 2

; Position accumulated since the driver started, 16-bit signed,
; little-endian, WITHOUT any clamping applied: the frame of reference
; (screen bounds, map coordinates...) is up to the caller, not this
; low-level driver, until one is defined by the rest of the game (see
; Plan initial.md in dune-cpc).
mouse_x: defw 0
mouse_y: defw 0

; Button state as of the last update, for edge detection in
; mouse_left_click_evt/mouse_right_click_evt below.
mouse_buttons: defb 0
mouse_previous_buttons: defb 0

; --- Raw reads -----------------------------------------------------------

; Reads and consumes the X delta — signed byte (see Mouse::read_dx on the
; ByteBox side: reading resets the counter to zero on the core side, so
; calling this twice in a row without motion returns 0 the second time).
; Output: A = delta, signed. Clobbers: F.
mouse_read_dx:
    ld   bc, MOUSE_PORT_DX
    in   a, (c)
    ret

; See mouse_read_dx.
mouse_read_dy:
    ld   bc, MOUSE_PORT_DY
    in   a, (c)
    ret

; Reads the CURRENT button state (not consumed on the ByteBox side, unlike
; the deltas: two consecutive reads with no change return the same
; value). Output: A = bit0 left, bit1 right (see the MOUSE_MASK_*
; constants). Clobbers: F.
mouse_read_buttons:
    ld   bc, MOUSE_PORT_BUTTONS
    in   a, (c)
    ret

; --- Driver state ----------------------------------------------------------

; Adds the signed byte in A to the little-endian 16-bit word pointed to by
; HL. Factored out once: mouse_update uses it for both X and Y. Preserves
; HL and DE. Clobbers: A, F.
add_signed_to_word:
    push de
    ld   e, a
    add  a, a          ; bit 7 (sign) of the original value -> carry
    sbc  a, a          ; A = #00 (positive/zero) or #FF (negative)
    ld   d, a
    ld   a, (hl)
    add  a, e
    ld   (hl), a
    inc  hl
    ld   a, (hl)
    adc  a, d
    ld   (hl), a
    dec  hl
    pop  de
    ret

; Main entry point of the driver: reads all three registers and updates
; the whole module state in one go (mouse_x/mouse_y accumulated,
; mouse_buttons/mouse_previous_buttons shifted for edge detection). Call
; once per game loop iteration — no more (deltas would be lost between two
; close reads, harmless but pointless), no less (a delta too large for a
; signed byte saturates on the ByteBox side rather than wrapping, see
; Mouse::on_motion/read_dx).
; Returns nothing through registers: callers read the variables directly.
; Clobbers: A, F (and BC/HL in transit, but restored by the end).
mouse_update:
    call mouse_read_dx
    ld   hl, mouse_x
    call add_signed_to_word
    call mouse_read_dy
    ld   hl, mouse_y
    call add_signed_to_word

    ld   a, (mouse_buttons)
    ld   (mouse_previous_buttons), a
    call mouse_read_buttons
    ld   (mouse_buttons), a
    ret

; --- Edge detection (call after mouse_update) -----------------------------

; True (A != 0, and then A = MOUSE_MASK_LEFT) if the left button was just
; pressed since the previous mouse_update — not just "currently down",
; which would re-trigger on every call for as long as it's held. False
; (A = 0) otherwise, including on a release.
; Clobbers: B, F.
mouse_left_click_evt:
    ld   a, (mouse_buttons)
    and  MOUSE_MASK_LEFT
    ld   b, a
    ld   a, (mouse_previous_buttons)
    and  MOUSE_MASK_LEFT
    cp   b
    jr   nz, mouse_left_edge   ; different state -> edge (rising or falling)
    xor  a                     ; unchanged state (released or held) -> no event
    ret
mouse_left_edge:
    ld   a, b                  ; b = masked current state: MASK (rising edge) or 0 (falling)
    ret

; See mouse_left_click_evt.
mouse_right_click_evt:
    ld   a, (mouse_buttons)
    and  MOUSE_MASK_RIGHT
    ld   b, a
    ld   a, (mouse_previous_buttons)
    and  MOUSE_MASK_RIGHT
    cp   b
    jr   nz, mouse_right_edge
    xor  a
    ret
mouse_right_edge:
    ld   a, b
    ret
