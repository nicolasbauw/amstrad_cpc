## Manual for the debugger

A concrete example of combined usage:

1. You launch the emulator.
2. You open the graphical debugger alongside it using **`F12`**.
3. You enter a breakpoint command in the console (e.g., `b 0x0038` for the interrupt).
4. As soon as the breakpoint is hit, the emulator freezes, and the debug window displays the full electronic state.
5. You press **`F10`** several times: you see the disassembly print out in the console, and all register values ​​(PC, SP, A, B, C...) change color or update in real-time in the debug window.
6. You press **`Shift + F10`**: the emulator skips ahead by one video line, and you watch the `HSYNC Counter` and `current_line` update in real-time on the debugger screen.
7. You press a key on the CPC display: it instantly lights up on the keyboard matrix shown in the second window.
