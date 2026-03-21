import { useEffect, useRef } from 'react';

interface GrainColors {
  c1: number[];
  c2: number[];
  c3: number[];
  c4: number[];
}

interface WebGLGrainProps {
  colors?: GrainColors;
  spreadX?: number;
  spreadY?: number;
  contrast?: number;
  noiseFactor?: number;
  opacity?: number;
}

export const WebGLGrain = ({
  colors = {
    c1: [48, 48, 48],
    c2: [34, 34, 34],
    c3: [24, 24, 24],
    c4: [20, 20, 20],
  },
  spreadX = 0.35,
  spreadY = 1.1,
  contrast = 2.0,
  noiseFactor = 0.7,
  opacity = 1.0,
}: WebGLGrainProps) => {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const latestPropsRef = useRef({ colors, spreadX, spreadY, contrast, noiseFactor });
  const renderRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    latestPropsRef.current = { colors, spreadX, spreadY, contrast, noiseFactor };
    renderRef.current?.();
  }, [colors, spreadX, spreadY, contrast, noiseFactor]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    const vsSource = `
      attribute vec2 position;
      void main() {
        gl_Position = vec4(position, 0.0, 1.0);
      }
    `;

    const fsSource = `
      precision highp float;
      uniform vec2 resolution;
      uniform vec3 c1, c2, c3, c4;
      uniform float spreadX, spreadY, contrast, noiseFactor;

      float random(vec2 st) {
          return fract(sin(dot(st.xy, vec2(12.9898,78.233))) * 43758.5453123);
      }

      void main() {
          vec2 uv = gl_FragCoord.xy / resolution.xy;
          vec2 center = vec2(0.5, 1.0);
          vec2 p = uv - center;

          p.x *= (resolution.x / resolution.y) * spreadX;
          p.y *= spreadY;

          float dist = length(p);
          float noise = random(gl_FragCoord.xy);

          float s1 = smoothstep(0.0, 0.5, dist);
          float d1 = clamp(((s1 + (noise - 0.5) * noiseFactor) - 0.5) * contrast + 0.5, 0.0, 1.0);

          float s2 = smoothstep(0.2, 0.9, dist);
          float d2 = clamp(((s2 + (noise - 0.5) * noiseFactor) - 0.5) * contrast + 0.5, 0.0, 1.0);

          float s3 = smoothstep(0.6, 1.4, dist);
          float d3 = clamp(((s3 + (noise - 0.5) * noiseFactor) - 0.5) * contrast + 0.5, 0.0, 1.0);

          vec3 finalColor = mix(c1, c2, d1);
          finalColor = mix(finalColor, c3, d2);
          finalColor = mix(finalColor, c4, d3);

          gl_FragColor = vec4(finalColor, 1.0);
      }
    `;

    let cleanupScene: (() => void) | null = null;

    const initializeScene = () => {
      const gl = canvas.getContext('webgl') || canvas.getContext('experimental-webgl');
      if (!gl || !(gl instanceof WebGLRenderingContext)) return;

      const compileShader = (type: number, source: string) => {
        const shader = gl.createShader(type);
        if (!shader) return null;
        gl.shaderSource(shader, source);
        gl.compileShader(shader);
        if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
          console.error('Failed to compile WebGLGrain shader:', gl.getShaderInfoLog(shader));
          gl.deleteShader(shader);
          return null;
        }
        return shader;
      };

      const vs = compileShader(gl.VERTEX_SHADER, vsSource);
      const fs = compileShader(gl.FRAGMENT_SHADER, fsSource);
      if (!vs || !fs) return;

      const prog = gl.createProgram();
      if (!prog) return;
      gl.attachShader(prog, vs);
      gl.attachShader(prog, fs);
      gl.linkProgram(prog);
      if (!gl.getProgramParameter(prog, gl.LINK_STATUS)) {
        console.error('Failed to link WebGLGrain program:', gl.getProgramInfoLog(prog));
        gl.deleteProgram(prog);
        gl.deleteShader(vs);
        gl.deleteShader(fs);
        return;
      }
      gl.useProgram(prog);

      const buffer = gl.createBuffer();
      if (!buffer) {
        gl.deleteProgram(prog);
        gl.deleteShader(vs);
        gl.deleteShader(fs);
        return;
      }
      gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, -1, 1, 1, -1, 1, 1]), gl.STATIC_DRAW);

      const posAttr = gl.getAttribLocation(prog, 'position');
      gl.enableVertexAttribArray(posAttr);
      gl.vertexAttribPointer(posAttr, 2, gl.FLOAT, false, 0, 0);

      const resUni = gl.getUniformLocation(prog, 'resolution');
      const uC1 = gl.getUniformLocation(prog, 'c1');
      const uC2 = gl.getUniformLocation(prog, 'c2');
      const uC3 = gl.getUniformLocation(prog, 'c3');
      const uC4 = gl.getUniformLocation(prog, 'c4');
      const uSpreadX = gl.getUniformLocation(prog, 'spreadX');
      const uSpreadY = gl.getUniformLocation(prog, 'spreadY');
      const uContrast = gl.getUniformLocation(prog, 'contrast');
      const uNoiseFactor = gl.getUniformLocation(prog, 'noiseFactor');

      const render = () => {
        const rect = canvas.getBoundingClientRect();
        if (rect.width === 0 || rect.height === 0) return;

        const dpr = window.devicePixelRatio || 1;
        canvas.width = rect.width * dpr;
        canvas.height = rect.height * dpr;

        const active = latestPropsRef.current;

        gl.useProgram(prog);
        gl.bindBuffer(gl.ARRAY_BUFFER, buffer);
        gl.enableVertexAttribArray(posAttr);
        gl.vertexAttribPointer(posAttr, 2, gl.FLOAT, false, 0, 0);
        gl.viewport(0, 0, canvas.width, canvas.height);
        gl.uniform2f(resUni, canvas.width, canvas.height);
        gl.uniform3f(uC1, active.colors.c1[0] / 255, active.colors.c1[1] / 255, active.colors.c1[2] / 255);
        gl.uniform3f(uC2, active.colors.c2[0] / 255, active.colors.c2[1] / 255, active.colors.c2[2] / 255);
        gl.uniform3f(uC3, active.colors.c3[0] / 255, active.colors.c3[1] / 255, active.colors.c3[2] / 255);
        gl.uniform3f(uC4, active.colors.c4[0] / 255, active.colors.c4[1] / 255, active.colors.c4[2] / 255);
        gl.uniform1f(uSpreadX, active.spreadX);
        gl.uniform1f(uSpreadY, active.spreadY);
        gl.uniform1f(uContrast, active.contrast);
        gl.uniform1f(uNoiseFactor, active.noiseFactor);
        gl.drawArrays(gl.TRIANGLES, 0, 6);
        gl.flush();
      };

      const ro = new ResizeObserver(render);
      ro.observe(canvas);
      renderRef.current = render;
      render();

      cleanupScene = () => {
        renderRef.current = null;
        ro.disconnect();
        gl.deleteBuffer(buffer);
        gl.deleteProgram(prog);
        gl.deleteShader(vs);
        gl.deleteShader(fs);
      };
    };

    const handleContextLost = (event: Event) => {
      event.preventDefault();
      cleanupScene?.();
      cleanupScene = null;
    };

    const handleContextRestored = () => {
      cleanupScene?.();
      cleanupScene = null;
      initializeScene();
    };

    canvas.addEventListener('webglcontextlost', handleContextLost, false);
    canvas.addEventListener('webglcontextrestored', handleContextRestored, false);
    initializeScene();

    return () => {
      canvas.removeEventListener('webglcontextlost', handleContextLost, false);
      canvas.removeEventListener('webglcontextrestored', handleContextRestored, false);
      cleanupScene?.();
      cleanupScene = null;
    };
  }, []);

  return <canvas ref={canvasRef} style={{ opacity }} className="absolute inset-0 w-full h-full pointer-events-none z-0" />;
};
