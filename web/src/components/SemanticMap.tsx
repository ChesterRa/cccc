import { useEffect, useMemo } from "react";
import * as THREE from "three";
import type { SemanticZone, SemanticZoneKind } from "../utils/panoramaSemanticMap";

interface SemanticMapProps {
  zones: SemanticZone[];
  isDark: boolean;
  projectStatus?: string | null;
}

function darkenHex(hex: string, factor: number): string {
  const h = hex.replace("#", "");
  const r = Math.round(parseInt(h.substring(0, 2), 16) * (1 - factor));
  const g = Math.round(parseInt(h.substring(2, 4), 16) * (1 - factor));
  const b = Math.round(parseInt(h.substring(4, 6), 16) * (1 - factor));
  return `#${r.toString(16).padStart(2, "0")}${g.toString(16).padStart(2, "0")}${b.toString(16).padStart(2, "0")}`;
}

function lightenHex(hex: string, factor: number): string {
  const h = hex.replace("#", "");
  const r = parseInt(h.substring(0, 2), 16);
  const g = parseInt(h.substring(2, 4), 16);
  const b = parseInt(h.substring(4, 6), 16);
  const mix = (value: number) => Math.round(value + (255 - value) * factor);
  return `#${mix(r).toString(16).padStart(2, "0")}${mix(g).toString(16).padStart(2, "0")}${mix(b).toString(16).padStart(2, "0")}`;
}

function ZoneLabel({ primary, secondary, color, isDark, position }: {
  primary: string;
  secondary?: string;
  color: string;
  isDark: boolean;
  position: [number, number, number];
}) {
  const labelState = useMemo(() => {
    const dpr = 2;
    const primaryFs = 11 * dpr;
    const secondaryFs = 8 * dpr;
    const canvas = document.createElement("canvas");
    const measure = canvas.getContext("2d")!;
    measure.font = `700 ${primaryFs}px sans-serif`;
    const primaryWidth = measure.measureText(primary).width;
    measure.font = `500 ${secondaryFs}px sans-serif`;
    const secondaryWidth = secondary ? measure.measureText(secondary).width : 0;
    const padX = 14 * dpr;
    const padY = 8 * dpr;
    const width = Math.ceil(Math.max(primaryWidth, secondaryWidth) + padX * 2);
    const height = Math.ceil(primaryFs + (secondary ? secondaryFs + 6 * dpr : 0) + padY * 2);
    canvas.width = width;
    canvas.height = height;
    const ctx = canvas.getContext("2d")!;

    ctx.beginPath();
    ctx.roundRect(0, 0, width, height, 8 * dpr);
    ctx.fillStyle = isDark ? "rgba(15,23,42,0.82)" : "rgba(255,255,255,0.9)";
    ctx.fill();

    ctx.strokeStyle = color;
    ctx.lineWidth = 2 * dpr;
    ctx.stroke();

    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.font = `700 ${primaryFs}px sans-serif`;
    ctx.fillStyle = isDark ? "#f8fafc" : "#0f172a";
    ctx.fillText(primary, width / 2, secondary ? height / 2 - 8 * dpr : height / 2);

    if (secondary) {
      ctx.font = `500 ${secondaryFs}px sans-serif`;
      ctx.fillStyle = isDark ? "#cbd5e1" : "#475569";
      ctx.fillText(secondary, width / 2, height / 2 + 11 * dpr);
    }

    const tex = new THREE.CanvasTexture(canvas);
    tex.colorSpace = THREE.SRGBColorSpace;
    const worldW = Math.max(1.1, Math.min(2.2, width / dpr / 95));
    return { tex, scale: [worldW, worldW * (height / width)] as [number, number] };
  }, [primary, secondary, color, isDark]);

  useEffect(() => () => { labelState.tex.dispose(); }, [labelState.tex]);

  return (
    <sprite position={position} scale={[labelState.scale[0], labelState.scale[1], 1]}>
      <spriteMaterial map={labelState.tex} transparent depthWrite={false} />
    </sprite>
  );
}

function StatusBanner({ text, isDark }: { text: string; isDark: boolean }) {
  return (
    <ZoneLabel
      primary={text}
      color={isDark ? "#38bdf8" : "#2563eb"}
      isDark={isDark}
      position={[0, 0.7, 8.7]}
    />
  );
}

function GuidePaths({ isDark }: { isDark: boolean }) {
  const lineColor = isDark ? "#94a3b8" : "#cbd5e1";
  return (
    <group>
      <mesh position={[0, 0.022, 4.2]}>
        <boxGeometry args={[12.4, 0.01, 0.16]} />
        <meshStandardMaterial color={lineColor} transparent opacity={0.35} />
      </mesh>
      <mesh position={[0, 0.022, -4.2]}>
        <boxGeometry args={[12.4, 0.01, 0.16]} />
        <meshStandardMaterial color={lineColor} transparent opacity={0.22} />
      </mesh>
      <mesh position={[0, 0.022, 0]}>
        <boxGeometry args={[0.18, 0.01, 12.8]} />
        <meshStandardMaterial color={lineColor} transparent opacity={0.16} />
      </mesh>
    </group>
  );
}

function createPatternTexture(kind: SemanticZoneKind, baseColor: string, accentColor: string): THREE.CanvasTexture {
  const size = 128;
  const canvas = document.createElement("canvas");
  canvas.width = size;
  canvas.height = size;
  const ctx = canvas.getContext("2d")!;
  ctx.fillStyle = baseColor;
  ctx.fillRect(0, 0, size, size);
  ctx.strokeStyle = accentColor;
  ctx.fillStyle = accentColor;
  ctx.globalAlpha = 0.28;

  if (kind === "task") {
    for (let x = -size; x < size * 2; x += 18) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x + size, size);
      ctx.lineWidth = 6;
      ctx.stroke();
    }
  } else if (kind === "blocked") {
    for (let x = -size; x < size * 2; x += 22) {
      ctx.beginPath();
      ctx.moveTo(x, 0);
      ctx.lineTo(x + size * 0.6, size);
      ctx.lineWidth = 10;
      ctx.stroke();
    }
  } else if (kind === "idle") {
    for (let y = 12; y < size; y += 20) {
      for (let x = 12; x < size; x += 20) {
        ctx.beginPath();
        ctx.arc(x, y, 3, 0, Math.PI * 2);
        ctx.fill();
      }
    }
  } else if (kind === "offline") {
    for (let x = 0; x < size; x += 16) {
      ctx.fillRect(x, 0, 3, size);
    }
    for (let y = 0; y < size; y += 16) {
      ctx.fillRect(0, y, size, 3);
    }
  } else {
    for (let x = 10; x < size; x += 18) {
      ctx.fillRect(x, 12, 3, size - 24);
    }
  }

  const texture = new THREE.CanvasTexture(canvas);
  texture.colorSpace = THREE.SRGBColorSpace;
  texture.wrapS = THREE.RepeatWrapping;
  texture.wrapT = THREE.RepeatWrapping;
  texture.repeat.set(2, 2);
  return texture;
}

function PathSegment({ from, to, width, color, opacity = 0.7 }: {
  from: [number, number, number];
  to: [number, number, number];
  width: number;
  color: string;
  opacity?: number;
}) {
  const dx = to[0] - from[0];
  const dz = to[2] - from[2];
  const length = Math.sqrt(dx * dx + dz * dz);
  const angle = Math.atan2(dx, dz);
  const midX = (from[0] + to[0]) / 2;
  const midZ = (from[2] + to[2]) / 2;
  return (
    <mesh position={[midX, 0.026, midZ]} rotation={[0, angle, 0]}>
      <boxGeometry args={[width, 0.012, Math.max(0.2, length)]} />
      <meshStandardMaterial color={color} transparent opacity={opacity} />
    </mesh>
  );
}

function TerrainBackdrop({ isDark }: { isDark: boolean }) {
  const ridge = isDark ? "#1f3a1d" : "#6b8f5a";
  const forest = isDark ? "#1d4d46" : "#4f8c78";
  const water = isDark ? "#123a7a" : "#4f83cc";
  const sand = isDark ? "#6b5d2e" : "#b59d5a";
  return (
    <group>
      <mesh position={[-5.6, 0.012, -0.6]} rotation={[0, -0.22, 0]}>
        <boxGeometry args={[3.6, 0.014, 4.8]} />
        <meshStandardMaterial color={forest} transparent opacity={0.22} />
      </mesh>
      <mesh position={[5.5, 0.012, -0.4]} rotation={[0, 0.16, 0]}>
        <boxGeometry args={[3.4, 0.014, 4.6]} />
        <meshStandardMaterial color={water} transparent opacity={0.2} />
      </mesh>
      <mesh position={[0, 0.011, 6.2]}>
        <boxGeometry args={[10.4, 0.012, 1]} />
        <meshStandardMaterial color={ridge} transparent opacity={0.18} />
      </mesh>
      <mesh position={[0, 0.011, -6.4]}>
        <boxGeometry args={[8.8, 0.012, 0.6]} />
        <meshStandardMaterial color={sand} transparent opacity={0.16} />
      </mesh>
    </group>
  );
}

function EdgeLandmarks({ isDark }: { isDark: boolean }) {
  const stone = isDark ? "#475569" : "#94a3b8";
  const glow = isDark ? "#60a5fa" : "#2563eb";
  return (
    <group>
      {[
        [-6.7, 0.24, 5.8],
        [6.7, 0.24, 5.8],
        [-6.5, 0.24, -5.7],
        [6.5, 0.24, -5.7],
      ].map((pos, index) => (
        <group key={`landmark-${index}`} position={pos as [number, number, number]}>
          <mesh>
            <boxGeometry args={[0.28, 0.48, 0.28]} />
            <meshStandardMaterial color={stone} flatShading />
          </mesh>
          <mesh position={[0, 0.28, 0]}>
            <boxGeometry args={[0.18, 0.12, 0.18]} />
            <meshStandardMaterial color={glow} emissive={glow} emissiveIntensity={0.35} />
          </mesh>
        </group>
      ))}
    </group>
  );
}

function RoadNetwork({ zones, isDark }: { zones: SemanticZone[]; isDark: boolean }) {
  const road = isDark ? "#cbd5e1" : "#94a3b8";
  const roadSoft = isDark ? "#64748b" : "#cbd5e1";
  const hub: [number, number, number] = [0, 0, 0];
  const taskZones = zones.filter((zone) => zone.kind === "task");
  const supportZones = zones.filter((zone) => zone.kind !== "task");

  return (
    <group>
      <mesh position={[hub[0], 0.028, hub[2]]}>
        <cylinderGeometry args={[0.42, 0.42, 0.03, 24]} />
        <meshStandardMaterial color={road} transparent opacity={0.5} />
      </mesh>
      {taskZones.map((zone) => (
        <PathSegment key={`task-road-${zone.id}`} from={hub} to={zone.center} width={0.34} color={road} opacity={0.42} />
      ))}
      {supportZones.map((zone) => (
        <PathSegment key={`support-road-${zone.id}`} from={hub} to={zone.center} width={0.2} color={roadSoft} opacity={0.22} />
      ))}
    </group>
  );
}

function CornerPosts({ zone }: { zone: SemanticZone }) {
  const postColor = lightenHex(zone.color, 0.12);
  const glowColor = lightenHex(zone.color, 0.35);
  const offsetX = zone.size[0] / 2 - 0.28;
  const offsetZ = zone.size[1] / 2 - 0.28;
  const points: Array<[number, number, number]> = [
    [-offsetX, 0.16, -offsetZ],
    [offsetX, 0.16, -offsetZ],
    [-offsetX, 0.16, offsetZ],
    [offsetX, 0.16, offsetZ],
  ];

  return (
    <group>
      {points.map((position, index) => (
        <group key={`${zone.id}-corner-${index}`} position={position}>
          <mesh>
            <boxGeometry args={[0.12, 0.3, 0.12]} />
            <meshStandardMaterial color={postColor} flatShading />
          </mesh>
          <mesh position={[0, 0.2, 0]}>
            <boxGeometry args={[0.16, 0.06, 0.16]} />
            <meshStandardMaterial color={glowColor} emissive={glowColor} emissiveIntensity={0.4} />
          </mesh>
        </group>
      ))}
    </group>
  );
}

function ZoneDecor({ zone, isDark }: { zone: SemanticZone; isDark: boolean }) {
  const metal = isDark ? "#cbd5e1" : "#94a3b8";
  const wood = isDark ? "#475569" : "#64748b";
  const accent = lightenHex(zone.color, 0.25);

  if (zone.kind === "idle") {
    return (
      <group>
        {[-1.4, 1.4].map((x) => (
          <group key={`${zone.id}-bench-${x}`} position={[x, 0.11, 0.7]}>
            <mesh>
              <boxGeometry args={[1.3, 0.08, 0.32]} />
              <meshStandardMaterial color={wood} flatShading />
            </mesh>
            <mesh position={[0, 0.3, -0.12]}>
              <boxGeometry args={[1.3, 0.38, 0.08]} />
              <meshStandardMaterial color={metal} flatShading />
            </mesh>
          </group>
        ))}
      </group>
    );
  }

  if (zone.kind === "task") {
    return (
      <group>
        <mesh position={[0, 0.18, 0.75]}>
          <boxGeometry args={[0.46, 0.36, 0.46]} />
          <meshStandardMaterial color={accent} flatShading emissive={accent} emissiveIntensity={0.18} />
        </mesh>
        <mesh position={[0, 0.55, 0.75]}>
          <boxGeometry args={[0.08, 0.42, 0.08]} />
          <meshStandardMaterial color={metal} flatShading />
        </mesh>
      </group>
    );
  }

  if (zone.kind === "blocked") {
    return (
      <group>
        {[-0.8, 0.8].map((x) => (
          <group key={`${zone.id}-warn-${x}`} position={[x, 0.14, 0.8]}>
            <mesh>
              <boxGeometry args={[0.18, 0.28, 0.18]} />
              <meshStandardMaterial color={accent} flatShading emissive={accent} emissiveIntensity={0.22} />
            </mesh>
            <mesh position={[0, 0.12, 0]}>
              <boxGeometry args={[0.3, 0.04, 0.04]} />
              <meshStandardMaterial color="#fee2e2" />
            </mesh>
          </group>
        ))}
      </group>
    );
  }

  if (zone.kind === "foreman") {
    return (
      <group position={[0, 0.12, 0.65]}>
        <mesh>
          <boxGeometry args={[1.2, 0.2, 0.6]} />
          <meshStandardMaterial color={wood} flatShading />
        </mesh>
        <mesh position={[0, 0.18, -0.05]}>
          <boxGeometry args={[0.85, 0.12, 0.18]} />
          <meshStandardMaterial color={accent} emissive={accent} emissiveIntensity={0.18} />
        </mesh>
      </group>
    );
  }

  if (zone.kind === "offline") {
    return (
      <mesh position={[0, 0.16, 0.7]}>
        <boxGeometry args={[0.5, 0.3, 0.5]} />
        <meshStandardMaterial color={metal} flatShading transparent opacity={0.65} />
      </mesh>
    );
  }

  return null;
}

function ZoneTile({ zone, isDark }: { zone: SemanticZone; isDark: boolean }) {
  const surfaceColor = darkenHex(zone.color, isDark ? 0.18 : 0.08);
  const insetColor = darkenHex(zone.color, isDark ? 0.34 : 0.2);
  const borderColor = lightenHex(zone.color, isDark ? 0.08 : 0.18);
  const stripeColor = isDark ? "#e2e8f0" : "#ffffff";
  const haloColor = lightenHex(zone.color, 0.3);
  const textureColor = lightenHex(zone.color, isDark ? 0.2 : 0.12);
  const surfaceTexture = useMemo(
    () => createPatternTexture(zone.kind, surfaceColor, textureColor),
    [zone.kind, surfaceColor, textureColor],
  );

  useEffect(() => () => { surfaceTexture.dispose(); }, [surfaceTexture]);

  return (
    <group position={zone.center}>
      <mesh position={[0, 0.008, 0]}>
        <boxGeometry args={[zone.size[0] + 0.5, 0.015, zone.size[1] + 0.5]} />
        <meshStandardMaterial color={haloColor} transparent opacity={0.1} />
      </mesh>
      <mesh position={[0, 0.01, 0]} receiveShadow>
        <boxGeometry args={[zone.size[0] + 0.18, 0.04, zone.size[1] + 0.18]} />
        <meshStandardMaterial color={borderColor} flatShading />
      </mesh>
      <mesh position={[0, 0.05, 0]} receiveShadow>
        <boxGeometry args={[zone.size[0], 0.06, zone.size[1]]} />
        <meshStandardMaterial color="#ffffff" map={surfaceTexture} flatShading transparent opacity={0.92} />
      </mesh>
      <mesh position={[0, 0.085, 0]}>
        <boxGeometry args={[Math.max(0.8, zone.size[0] - 0.7), 0.01, Math.max(0.8, zone.size[1] - 0.7)]} />
        <meshStandardMaterial color={insetColor} transparent opacity={0.45} />
      </mesh>
      <mesh position={[0, 0.09, zone.size[1] / 2 - 0.22]}>
        <boxGeometry args={[zone.size[0] - 0.8, 0.02, 0.14]} />
        <meshStandardMaterial color={stripeColor} emissive={stripeColor} emissiveIntensity={0.18} />
      </mesh>
      <mesh position={[0, 0.09, -zone.size[1] / 2 + 0.22]}>
        <boxGeometry args={[zone.size[0] - 1.2, 0.015, 0.08]} />
        <meshStandardMaterial color={stripeColor} emissive={stripeColor} emissiveIntensity={0.2} />
      </mesh>
      <CornerPosts zone={zone} />
      <ZoneDecor zone={zone} isDark={isDark} />
      <ZoneLabel
        primary={zone.label}
        secondary={zone.subtitle}
        color={zone.color}
        isDark={isDark}
        position={[0, 0.58, 0]}
      />
    </group>
  );
}

export function SemanticMap({ zones, isDark, projectStatus }: SemanticMapProps) {
  return (
    <>
      <TerrainBackdrop isDark={isDark} />
      <RoadNetwork zones={zones} isDark={isDark} />
      <GuidePaths isDark={isDark} />
      <EdgeLandmarks isDark={isDark} />
      {zones.map((zone) => (
        <ZoneTile key={zone.id} zone={zone} isDark={isDark} />
      ))}
      {projectStatus ? <StatusBanner text={projectStatus} isDark={isDark} /> : null}
    </>
  );
}
