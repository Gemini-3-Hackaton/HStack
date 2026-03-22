import type { CSSProperties } from 'react';

export interface GrainColors {
  c1: number[];
  c2: number[];
  c3: number[];
  c4: number[];
}

const rgba = (color: number[], alpha: number) => `rgba(${color[0]}, ${color[1]}, ${color[2]}, ${alpha})`;
const rgb = (color: number[]) => `rgb(${color[0]}, ${color[1]}, ${color[2]})`;

interface GrainFallbackProps {
  colors: GrainColors;
  opacity?: number;
  animated?: boolean;
}

export const GrainFallback = ({ colors, opacity = 1, animated = false }: GrainFallbackProps) => {
  const atmosphereStyle: CSSProperties = {
    backgroundColor: rgb(colors.c4),
    backgroundImage: [
      `radial-gradient(140% 95% at 50% 100%, ${rgba(colors.c1, 0.96)} 0%, ${rgba(colors.c2, 0.88)} 34%, ${rgba(colors.c3, 0.7)} 62%, ${rgba(colors.c4, 0.94)} 100%)`,
      `radial-gradient(85% 60% at 18% 12%, ${rgba(colors.c2, 0.22)} 0%, transparent 72%)`,
      `radial-gradient(72% 58% at 82% 18%, ${rgba(colors.c1, 0.16)} 0%, transparent 74%)`,
      `linear-gradient(180deg, ${rgba(colors.c3, 0.18)} 0%, ${rgba(colors.c4, 0.06)} 100%)`,
    ].join(', '),
  };

  const noiseStyle: CSSProperties = {
    backgroundImage: [
      'repeating-linear-gradient(0deg, rgba(255,255,255,0.05) 0px, rgba(255,255,255,0.05) 1px, transparent 1px, transparent 3px)',
      'repeating-linear-gradient(90deg, rgba(0,0,0,0.06) 0px, rgba(0,0,0,0.06) 1px, transparent 1px, transparent 4px)',
      'linear-gradient(135deg, rgba(255,255,255,0.035), rgba(0,0,0,0.08))',
    ].join(', '),
  };

  return (
    <div aria-hidden="true" className="absolute inset-0 pointer-events-none z-0 overflow-hidden" style={{ opacity }}>
      <div
        className={`absolute inset-0 scale-[1.04] ${animated ? 'grain-fallback-shift' : ''}`}
        style={atmosphereStyle}
      />
      <div className={`absolute inset-0 mix-blend-soft-light ${animated ? 'grain-fallback-noise' : ''}`} style={noiseStyle} />
      <div className="absolute inset-0 bg-[radial-gradient(circle_at_top,rgba(255,255,255,0.06),transparent_45%)] opacity-50" />
    </div>
  );
};