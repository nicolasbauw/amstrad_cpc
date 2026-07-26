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
