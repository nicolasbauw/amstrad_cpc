// Shader CRT (F5, Plan V2.md jalon M4) : scanlines + masque phosphore,
// inspiré de la famille Lottes/"easymode" (pas de distorsion en barillet,
// non demandée). Même quad plein écran que renderer_frame.wgsl ; c'est
// renderer.rs qui choisit l'un ou l'autre pipeline selon F5.
//
// Deux effets, deux espaces de calcul délibérément différents :
//
// - Les bandes de balayage correspondent à une ligne du SIGNAL VIDÉO : leur
//   nombre doit rester celui de l'image source (SCREEN_HEIGHT lignes) quel
//   que soit le zoom, donc leur calcul se fait en espace pixel SOURCE
//   (uv * source_size). Une pitch fixe en pixels d'écran donnerait un
//   nombre de bandes différent à x1 et à x3.
//
// - Le masque (grille d'ouverture/masque perforé) est au contraire une
//   texture du TUBE lui-même : sur un vrai CRT, sa finesse est fixée par la
//   fabrication de l'écran, sans aucun rapport avec la résolution de
//   l'image qu'il affiche. Le calculer en espace pixel source (comme le
//   premier essai de ce shader) donnait un masque aussi grossier que les
//   pixels du CPC — de gros carrés adoucis, pas un CRT. Il se calcule donc
//   en espace pixel de SORTIE réel (`@builtin(position)`), à une taille de
//   cellule fixe en pixels d'écran.

struct CrtParams {
    source_size: vec2<f32>,
    _padding: vec2<f32>,
};

@group(0) @binding(0)
var t_frame: texture_2d<f32>;
@group(0) @binding(1)
var s_frame: sampler;
@group(1) @binding(0)
var<uniform> params: CrtParams;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 4>(
        vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(1.0, 1.0),
        vec2<f32>(1.0, -1.0),
    );
    var uvs = array<vec2<f32>, 4>(
        vec2<f32>(0.0, 0.0),
        vec2<f32>(0.0, 1.0),
        vec2<f32>(1.0, 0.0),
        vec2<f32>(1.0, 1.0),
    );
    var out: VertexOutput;
    out.clip_position = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

// Intensité de l'assombrissement entre les lignes de balayage : 0 = aucun
// effet, 1 = noir complet au bord de chaque ligne source.
const SCANLINE_STRENGTH: f32 = 0.4;

// Taille d'une cellule du masque phosphore, en pixels de sortie réels —
// c'est ce nombre, pas la résolution source, qui donne sa finesse au
// masque. Plus petit = grille plus fine (mais plus coûteuse à distinguer
// sans un zoom suffisant, voir plus bas). Volontairement non entier :
// à une période entière, un fragment shader qui échantillonne pile au
// centre de chaque pixel de sortie retombe exactement en phase d'une
// colonne à l'autre — le motif s'annule au lieu de se dessiner (constaté
// avec 2.0 : tous les fragments tombaient à la même distance du centre de
// leur cellule, donnant un assombrissement uniforme plutôt qu'une grille).
const MASK_PERIOD: f32 = 2.3;
// Luminosité minimale entre deux points du masque : ne descend jamais au
// noir complet, sous peine d'un maillage trop dur plutôt qu'un grain discret.
const MASK_MIN: f32 = 0.55;

// Compense la perte de luminosité moyenne qu'entraînent les deux effets
// ci-dessus : sans ça, l'image paraît plus sombre une fois le shader actif.
const GAIN: f32 = 1.2;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_frame, s_frame, in.uv).rgb;

    // Bandes de balayage : chaque ligne source est la plus lumineuse en son
    // centre vertical, et s'assombrit vers le haut/bas — un cosinus plutôt
    // qu'un découpage net, pour une transition sans crénelage au zoom élevé.
    let row_frac = fract(in.uv.y * params.source_size.y) - 0.5;
    let scan = mix(1.0, cos(row_frac * 3.14159265) * 0.5 + 0.5, SCANLINE_STRENGTH);

    // Masque phosphore : petits points, en grille régulière sur l'écran
    // réel (voir le commentaire en tête de fichier). Un produit de cosinus
    // plutôt qu'une distance à un centre de cellule (`fract` + `smoothstep`,
    // le premier essai) : sans discontinuité à la frontière des cellules,
    // moins sujet à l'aliasing qui annulait le motif à MASK_PERIOD entier.
    let phase = in.clip_position.xy * (6.2831853 / MASK_PERIOD);
    let dot = cos(phase.x) * cos(phase.y); // 1.0 au centre d'un point, -1.0 entre deux
    let mask = mix(1.0, MASK_MIN, smoothstep(-0.2, 0.6, -dot));

    return vec4<f32>(color * scan * mask * GAIN, 1.0);
}
