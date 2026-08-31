//! Identifiants de touche propres à ByteBox, indépendants de toute boîte à
//! outils de fenêtrage.
//!
//! `core` n'a aucune raison de connaître SDL2 (ou, demain, `winit`, ou les
//! événements clavier d'un navigateur) : c'est une dépendance de
//! présentation, et `core` doit rester utilisable par n'importe quelle
//! façade. `Psg::set_key_state`/`set_key_state_scancode` prennent donc les
//! types définis ici, et c'est à CHAQUE façade (SDL2 aujourd'hui, une
//! éventuelle façade web demain) de traduire ses propres événements vers ces
//! deux énumérations avant d'appeler `core`.
//!
//! Noms calqués sur ceux de SDL2 (`Keycode`/`Scancode`) pour que cette
//! traduction reste une table de correspondance quasi mécanique, pas une
//! réinterprétation — mais ce ne sont plus les types de SDL2 : rien n'y
//! contraint une future façade à s'y conformer au-delà du nom. Seules les
//! variantes réellement utilisées par la matrice clavier du CPC (voir
//! `psg.rs`) sont présentes ; en ajouter une plus tard est un changement
//! rétrocompatible, tant que le code appelant garde un `_ => None` (ce que
//! `psg.rs` fait déjà).
//!
//! `Keycode` correspond au CARACTÈRE produit par la disposition active (ce
//! que la plupart des touches veulent atteindre) ; `Scancode` correspond à
//! la POSITION physique de la touche, indépendante de la disposition et du
//! SHIFT — nécessaire pour les quelques touches où `Keycode` n'est pas
//! fiable sur macOS (voir le commentaire de
//! `Psg::set_key_state_scancode`).

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Keycode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Up,
    Down,
    Left,
    Right,
    Return,
    Backspace,
    Delete,
    Escape,
    Tab,
    Space,
    CapsLock,
    LShift,
    RShift,
    LCtrl,
    RCtrl,
    LAlt,
    RAlt,
    Minus,
    Equals,
    Plus,
    Colon,
    Slash,
    Percent,
    Comma,
    Period,
    Semicolon,
    RightParen,
    Kp0,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    KpEnter,
    KpPeriod,
    KpMultiply,
    KpDivide,
    KpEquals,
    KpPlus,
    KpMinus,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Scancode {
    Apostrophe,
    LeftBracket,
    Grave,
    RightBracket,
    NonUsBackslash,
    Num8,
}
