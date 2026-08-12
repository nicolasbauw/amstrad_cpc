# ByteBox - an Amstrad CPC 6128 Emulator

## Command-line options

- `--diag` : additional diagnostics ROM (0F slot)
- `--disk=<file>` / `-d <file>`: load a disk image on drive A at startup. A bare
  filename (no path) is looked up in `[file] dsk_path` from `config.toml` if it
  isn't found as given.
- `--autocmd=<command>` / `-a <command>`: type a command at the emulated keyboard
  once BASIC is ready, exactly like Caprice32's own `--autocmd`. Handy for
  jumping straight into a game during a debugging session:

  ```sh
  cargo run -- --disk=Barbarian.dsk --autocmd='RUN"BARBA.I'
  ```
  
## Emulator commands:
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

## Tape drive

The emulator reproduces a real datacorder: loading `.cdt` images (see the
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
  
## Emulator monitor commands:
  
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

1. You launch the emulator.
2. You open the machine status window alongside it using **`F12`**.
3. You enter a breakpoint command in the console (e.g., `b 0x0038` for the interrupt).
4. As soon as the breakpoint is hit, the emulator freezes, and the debug window displays the full machine state.
5. You press **`F10`** several times: you see the disassembly print out in the console, and all register values ​​(PC, SP, A, B, C...) update in the debug window.
6. You press **`Shift + F10`**: the emulator skips ahead by one video line, and you watch the `HSYNC Counter` and `current_line` update in real-time on the debugger screen.

![Screenshot](assets/machine_status.png)
