- pourquoi la rom de diag commence par l'itération 222 (0xDE) (stocké en RAM adresse 0x81f0)
- pourquoi la rom de diag détecte des ROMs UNKNOWN
- finir l'interfaçage du clavier avec le reste
- Après le test des ROMs il y a un message "press any key" qui passe sans action sur le clavier
- se renseigner sur le fonctionnement du BORDER, et l'implémenter


> b 0x193e
New breakpoint at 0x193E
>
Breakpoint reached at 0x193E (Total Ticks: 15027995)
r
=== REGISTERS & STATUS ===
PC :0x193E   SP : 0xBFFF
S : 1  Z : 0  H : 0  P : 1  N : 0  C : 0
B : 0x00  C : 0xFF  D : 0x81  E : 0xD2  H : 0xC7  L : 0xD0  A : 0xDD
(SP) : 0x0000  IFF1 : false  IFF2 : false  IM : 1  Pending INT : true  Pending NMI : false
> d 0x193e
193E    32            LD ($81F0),A  <-- De là vient le 0xDD
1941    CD CB 1F      CALL $1FCB
1944    C3 59 19      JP $1959
1947    21 B1 35      LD HL,$35B1
194A    11 00 80      LD DE,$8000
194D    01 D2 01      LD BC,$01D2
1950    Unknown opcode              <-- LDIR (implémenté mais pas dans le désassembleur)

Watchpoint hit: write to 0x81F0 at PC 0x1FAB
32            LD ($81F0),A
=== REGISTERS & STATUS ===
PC :0x1FAE   SP : 0xBFFF
S : 1  Z : 0  H : 0  P : 0  N : 0  C : 0
B : 0x04  C : 0xFF  D : 0x81  E : 0xD2  H : 0xC7  L : 0xD0  A : 0xDE  <-- le fameux 0xDE (222)
(SP) : 0x0000  IFF1 : false  IFF2 : false  IM : 1  Pending INT : true  Pending NMI : false


1F82    D6 81         SUB A,$81
1F84    C9            RET
1F85    DD21 EC 81    LD IX,nn
1F89    06 04         LD B,$04
1F8B    DD7E 00       LD A,(IX+d)
1F8E    FE 53         CP $53
1F90    C0            RET NZ
1F91    DD7E 01       LD A,(IX+d)
1F94    FE 4F         CP $4F
1F96    C0            RET NZ
1F97    DD7E 02       LD A,(IX+d)
1F9A    FE 41         CP $41
1F9C    C0            RET NZ
1F9D    DD7E 03       LD A,(IX+d)
1FA0    FE 4B         CP $4B
1FA2    C0            RET NZ
1FA3    C9            RET
1FA4    CD CB 1F      CALL $1FCB
1FA7    3A F0 81      LD A,($81F0)    0xDD
1FAA    3C            INC A
1FAB    32            LD ($81F0),A    0xDE


1FCB    DD21 EC 81    LD IX,nn
1FCF    DD36 00 53    LD (IX+d),n
1FD3    DD36 01 4F    LD (IX+d),n
1FD7    DD36 02 41    LD (IX+d),n
1FDB    DD36 03 4B    LD (IX+d),n
1FDF    C9            RET



> d 0x1930
1930    F6 ED         OR $ED
1932    49            LD C,C
1933    01 00 10      LD BC,$1000
1936    0B            DEC BC
1937    78            LD A,B
1938    B1            OR C
1939    20 FB         JR NZ,$1936
193B    C3 59 19      JP $1959
193E    32            LD ($81F0),A  <-- De là vient le 0xDD
1941    CD CB 1F      CALL $1FCB
1944    C3 59 19      JP $1959
1947    21 B1 35      LD HL,$35B1
194A    11 00 80      LD DE,$8000
194D    01 D2 01      LD BC,$01D2
1950    EDB0          LDIR
1952    CD D4 24      CALL $24D4
1955    CD 47 2E      CALL $2E47
1958    C9            RET
1959    CD 85 1F      CALL $1F85
195C    CA A4 1F      JP Z,$1FA4
195F    CD FA 2C      CALL $2CFA
> d 0x195f
195F    CD FA 2C      CALL $2CFA
1962    3A D7 81      LD A,($81D7)
1965    B7            OR A
1966    20 03         JR NZ,$196B
1968    CD B6 1D      CALL $1DB6
196B    21 70 82      LD HL,$8270
196E    CD 2A 29      CALL $292A
1971    CD 5F 1B      CALL $1B5F
1974    CD FA 1C      CALL $1CFA
1977    CD F4 24      CALL $24F4
197A    CD 48 25      CALL $2548
197D    CD 83 25      CALL $2583
1980    DD21 AE 81    LD IX,nn
1984    3A AC 81      LD A,($81AC)
1987    47            LD B,A
1988    0E 01         LD C,$01
198A    21 5C 82      LD HL,$825C
198D    16 00         LD D,$00
198F    DD5E 04       LD E,(IX+d)
1992    19            ADD HL,DE
1993    7E            LD A,(HL)
> d 0x1993
1993    7E            LD A,(HL)
1994    DD56 05       LD D,(IX+d)
1997    A2            AND D
1998    B7            OR A
1999    C2 D3 19      JP NZ,$19D3
199C    11 06 00      LD DE,$0006
199F    DD19          ADD IX,DE
19A1    CB21          SLA C
19A3    10 E5         DJNZ $198A
19A5    3A 5C 82      LD A,($825C)
19A8    CB47          BIT 0,A
19AA    20 2E         JR NZ,$19DA
19AC    CB57          BIT 2,A
19AE    20 46         JR NZ,$19F6
19B0    CB77          BIT 6,A
19B2    20 65         JR NZ,$1A19
19B4    3A 5E 82      LD A,($825E)
19B7    CB57          BIT 2,A
19B9    20 5E         JR NZ,$1A19
19BB    3A 61 82      LD A,($8261)
19BE    CB7F          BIT 7,A
