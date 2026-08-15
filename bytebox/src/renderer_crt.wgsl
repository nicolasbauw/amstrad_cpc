// Shader CRT (F5, Plan V2.md jalon M4) : scanlines + aperture arrondie des
// pixels, inspiré de la famille Lottes/"easymode" (pas de distorsion en
// barillet, non demandée). Même quad plein écran que renderer_frame.wgsl ;
// c'est renderer.rs qui choisit l'un ou l'autre pipeline selon F5.

struct CrtParams {
    // Taille de l'image source (SCREEN_WIDTH x SCREEN_HEIGHT), en pixels
    // *source*, jamais en pixels de sortie : c'est ce qui garantit un
    // rendu aux mêmes proportions en x1/x2/x3/plein écran, plutôt que des
    // scanlines à l'espacement fixe en pixels d'écran (qui donnerait un
    // nombre de bandes différent selon le zoom).
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
const SCANLINE_STRENGTH: f32 = 0.45;
// Largeur de la transition vers les coins de chaque pixel (aperture ronde
// du faisceau plutôt qu'un carré net) : plus grand = transition plus douce.
const MASK_SOFTNESS: f32 = 0.5;
// Luminosité minimale aux coins d'un pixel : évite qu'ils tombent au noir
// complet, ce qui donnerait une grille trop marquée façon moustiquaire
// plutôt qu'un adoucissement discret.
const MASK_MIN: f32 = 0.78;
// Compense la perte de luminosité moyenne qu'entraînent les deux effets
// ci-dessus : sans ça, l'image paraît plus sombre une fois le shader actif.
const GAIN: f32 = 1.12;

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let color = textureSample(t_frame, s_frame, in.uv).rgb;

    // Position dans le pixel source courant, centrée sur (0,0), en unités
    // de pixel *source* — jamais de pixel de sortie, voir `CrtParams`.
    let texel = in.uv * params.source_size;
    let frac = fract(texel) - vec2<f32>(0.5);

    // Aperture ronde : le faisceau électronique est un point, pas un
    // carré — la luminosité retombe doucement vers les coins de chaque
    // pixel plutôt que de s'arrêter net à ses bords. Plafonnée à MASK_MIN
    // plutôt que d'aller jusqu'au noir : un adoucissement, pas une grille.
    let corner_dist = length(frac) * 2.0;
    let aperture = mix(1.0, MASK_MIN, smoothstep(1.0 - MASK_SOFTNESS, 1.0, corner_dist));

    // Ligne de balayage : chaque pixel source est le plus lumineux en son
    // centre vertical, et s'assombrit vers le haut/bas — un cosinus plutôt
    // qu'un découpage net, pour une transition sans crénelage au zoom élevé.
    let scan = mix(1.0, cos(frac.y * 3.14159265) * 0.5 + 0.5, SCANLINE_STRENGTH);

    return vec4<f32>(color * aperture * scan * GAIN, 1.0);
}
