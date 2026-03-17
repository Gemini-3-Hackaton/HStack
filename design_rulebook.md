Engraved Dark Technical Design Rulebook

1. Core Philosophy: The Physical Material

The "Engraved Dark" system is based on Coplanar Physics. It rejects the
"elevation" model of traditional UI (where things float) in favor of a
"subtractive" model. Every element is either a flat sheet or an excavation
(engraving) into the base material.

The "Moat" Architecture

To create the illusion of an engraved tile, you must nest two containers:

The Trench (Outer): Acts as the hole in the ground.

Background: #141414

Shadow: inset 0 2px 5px rgba(0,0,0,0.8) (Carves inward).

The Tile (Inner): Acts as the material placed inside.

Background: Handled by WebGL (Base color #121212).

Shadow: 0 2px 5px rgba(0,0,0,0.7) (Casts shadow into the trench).

Padding: A consistent 4px "Ridge" must exist between the two.

2. Lighting & Physics Rules

Light is radial, originating from the Top-Middle of the tile.

No Blurry Glows: Lighting must feel like it’s interacting with a physical
texture.

Zero-Blur Creases: Use sharp 1px or 2px highlights at the top-left edges only.

Uniform Flatness: Tile surfaces must be mathematically flat. Do not use linear
gradients for the surface; use the Dither Shader for all transitions.

3. The WebGL Dither Shader (Technical Spec)

This is the system's engine. It replaces smooth gradients with dithered grain.

Fragment Shader Code (GLSL)

precision highp float; uniform vec2 resolution; uniform vec3 c1, c2, c3, c4; //
Multi-step colors

float random(vec2 st) { return fract(sin(dot(st.xy, vec2(12.9898,78.233))) *
43758.5453123); }

void main() { vec2 uv = gl_FragCoord.xy / resolution.xy; vec2 p = uv - vec2(0.5,
1.0); // Source at Top-Middle

    // Physical Rule: Elongate radius horizontally to match card aspect ratios
    p.x *= (resolution.x / resolution.y) * 0.35; 
    p.y *= 1.1; 

    float dist = length(p); 
    float noise = random(gl_FragCoord.xy);

    // Diffusion Rule: Low contrast (2.0) and high noise influence (0.7)
    float contrast = 2.0; 
    float nFact = 0.7;

    // 3-Step Multi-Dither
    float s1 = smoothstep(0.0, 0.5, dist); 
    float d1 = clamp(((s1 + (noise - 0.5) * nFact) - 0.5) * contrast + 0.5, 0.0, 1.0);

    float s2 = smoothstep(0.2, 0.9, dist);
    float d2 = clamp(((s2 + (noise - 0.5) * nFact) - 0.5) * contrast + 0.5, 0.0, 1.0);

    float s3 = smoothstep(0.6, 1.4, dist);
    float d3 = clamp(((s3 + (noise - 0.5) * nFact) - 0.5) * contrast + 0.5, 0.0, 1.0);

    vec3 color = mix(c1, c2, d1);
    color = mix(color, c3, d2);
    color = mix(color, c4, d3);

    gl_FragColor = vec4(color, 1.0);

}

4. Color Palette (Stealth Mode)

Colors are neutralized grays with a "ghost" tint.

Layer

Color

Purpose

App BG

#080808

Base environment.

Moat BG

#141414

The "dug out" floor.

Tile Base

#121212

Bottom of the card.

Primary Text

#D1D1D1

High legibility, no pure white.

Meta Text

#777777

Subdued information.

Theme Palettes (RGB 0-255)

Default: C1:[48,48,48], C4:[20,20,20]

Habit: C1:[42,52,48], C4:[20,20,20] (Subtle Emerald)

Event: C1:[54,48,40], C4:[20,20,20] (Subtle Amber)

5. Component Logic

The Physical Wrapper (CSS/Tailwind)

.moat { background: #141414; padding: 4px; box-shadow: inset 0 2px 5px
rgba(0,0,0,0.8); }

.tile { background: #121212; /* Fallback */ box-shadow: 0 2px 5px
rgba(0,0,0,0.7); overflow: hidden; }

The Tag Component

Every tag must be "carved" from the same material.

Rule: Tag border must be 1px solid with the color rgba(Parent_C1_Highlight,
0.25).

Spacing: px-1.5 py-1.

Typography: Bold, tracking-widest, 9px.

6. Layout Behavior

Vertical Elasticity: Cards must never have a fixed height. Use flex flex-wrap
for tag containers.

Gestural Focus: Standard affordances (Checkboxes/Toggle switches) are removed.
Interaction is implied through side-swiping or long-pressing.

Background Dither: The main app background must feature an ultra-diffuse version
of the shader (Contrast 1.2, Opacity 0.8) to prevent "flat void" syndrome.

7. Implementation Checklist

[ ] Does the light source originate from the top-middle?

[ ] Is the horizontal dither spread elongated enough?

[ ] Are the tag borders sampling the card's theme color?

[ ] Is the surface grain thresholded (mix + clamp + contrast) or just an
overlay? (Must be thresholded).
