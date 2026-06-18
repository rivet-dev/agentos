"use client";

import { useEffect, useRef } from "react";

interface Point {
  x: number;
  y: number;
}

interface Bolt {
  segments: Point[];
  alpha: number;
  decay: number;
  width: number;
  branches: Bolt[];
  flash: number;
}

function generateBolt(
  startX: number,
  startY: number,
  endX: number,
  endY: number,
  displacement: number,
  minSegLen: number
): Point[] {
  if (displacement < minSegLen) {
    return [{ x: startX, y: startY }, { x: endX, y: endY }];
  }

  const midX = (startX + endX) / 2 + (Math.random() - 0.5) * displacement;
  const midY = (startY + endY) / 2 + (Math.random() - 0.5) * displacement;

  const left = generateBolt(startX, startY, midX, midY, displacement / 2, minSegLen);
  const right = generateBolt(midX, midY, endX, endY, displacement / 2, minSegLen);

  return [...left.slice(0, -1), ...right];
}

function createBranch(pt: Point, angle: number, len: number, depth: number): Bolt {
  const bEndX = pt.x + Math.cos(angle) * len;
  const bEndY = pt.y + Math.sin(angle) * len;
  const bSegments = generateBolt(pt.x, pt.y, bEndX, bEndY, len * 0.45, 5);

  const subBranches: Bolt[] = [];
  if (depth < 2 && Math.random() < 0.5) {
    const subIdx = Math.floor(Math.random() * (bSegments.length - 2)) + 1;
    const subPt = bSegments[subIdx];
    const subAngle = angle + (Math.random() - 0.5) * 1.8;
    const subLen = len * (0.3 + Math.random() * 0.3);
    subBranches.push(createBranch(subPt, subAngle, subLen, depth + 1));
  }

  return {
    segments: bSegments,
    alpha: 0.8,
    decay: 0.05 + Math.random() * 0.03,
    width: Math.max(0.5, 1.5 - depth * 0.4),
    branches: subBranches,
    flash: 0,
  };
}

function createBolt(w: number, h: number): Bolt {
  const side = Math.random();
  let startX: number, startY: number, endX: number, endY: number;

  if (side < 0.25) {
    startX = Math.random() * w;
    startY = 0;
    endX = startX + (Math.random() - 0.5) * w * 0.8;
    endY = h * (0.4 + Math.random() * 0.6);
  } else if (side < 0.4) {
    startX = 0;
    startY = Math.random() * h * 0.4;
    endX = w * (0.3 + Math.random() * 0.6);
    endY = startY + Math.random() * h * 0.6;
  } else if (side < 0.55) {
    startX = w;
    startY = Math.random() * h * 0.4;
    endX = w * (0.1 + Math.random() * 0.6);
    endY = startY + Math.random() * h * 0.6;
  } else if (side < 0.8) {
    startX = w * (0.25 + Math.random() * 0.5);
    startY = h * (0.15 + Math.random() * 0.35);
    const angle = Math.random() * Math.PI * 2;
    const dist = Math.min(w, h) * (0.35 + Math.random() * 0.5);
    endX = startX + Math.cos(angle) * dist;
    endY = startY + Math.sin(angle) * dist;
  } else {
    const fromLeft = Math.random() < 0.5;
    startX = fromLeft ? w * Math.random() * 0.15 : w * (0.85 + Math.random() * 0.15);
    startY = Math.random() * h * 0.2;
    endX = fromLeft ? w * (0.6 + Math.random() * 0.4) : w * Math.random() * 0.4;
    endY = h * (0.6 + Math.random() * 0.4);
  }

  const displacement = Math.hypot(endX - startX, endY - startY) * 0.4;
  const segments = generateBolt(startX, startY, endX, endY, displacement, 5);

  const branches: Bolt[] = [];
  const branchCount = Math.floor(Math.random() * 4) + 2;
  const mainAngle = Math.atan2(endY - startY, endX - startX);

  for (let i = 0; i < branchCount; i++) {
    const idx = Math.floor(Math.random() * (segments.length - 2)) + 1;
    const pt = segments[idx];
    const angle = mainAngle + (Math.random() - 0.5) * 2.0;
    const len = Math.min(w, h) * (0.1 + Math.random() * 0.2);
    branches.push(createBranch(pt, angle, len, 0));
  }

  return {
    segments,
    alpha: 1,
    decay: 0.04 + Math.random() * 0.03,
    width: 2 + Math.random() * 1.5,
    branches,
    flash: 0,
  };
}

// Trace a path without stroking — reused across passes
function tracePath(ctx: CanvasRenderingContext2D, segs: Point[]) {
  ctx.moveTo(segs[0].x, segs[0].y);
  for (let i = 1; i < segs.length; i++) {
    ctx.lineTo(segs[i].x, segs[i].y);
  }
}

// Collect all segments from a bolt tree into flat arrays by width tier
function collectSegments(
  bolt: Bolt,
  parentAlpha: number,
  glow: Point[][],
  glowAlphas: number[],
  core: Point[][],
  coreAlphas: number[],
  coreWidths: number[]
) {
  const a = Math.min(bolt.alpha, parentAlpha);
  if (a <= 0 || bolt.segments.length < 2) return;

  glow.push(bolt.segments);
  glowAlphas.push(a);
  core.push(bolt.segments);
  coreAlphas.push(a);
  coreWidths.push(bolt.width);

  for (const branch of bolt.branches) {
    collectSegments(branch, a * 0.55, glow, glowAlphas, core, coreAlphas, coreWidths);
  }
}

export function LightningBackground() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const boltsRef = useRef<Bolt[]>([]);
  const animFrameRef = useRef<number>(0);
  const flashRef = useRef<number>(0);
  const sizeRef = useRef({ w: 0, h: 0, dpr: 1 });

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d", { alpha: true });
    if (!ctx) return;

    const resize = () => {
      const dpr = window.devicePixelRatio || 1;
      const rect = canvas.parentElement?.getBoundingClientRect();
      if (!rect) return;
      canvas.width = rect.width * dpr;
      canvas.height = rect.height * dpr;
      canvas.style.width = `${rect.width}px`;
      canvas.style.height = `${rect.height}px`;
      sizeRef.current = { w: rect.width, h: rect.height, dpr };
    };

    resize();
    window.addEventListener("resize", resize);

    const spawnedRef = { done: false };
    const startTime = Date.now();

    const animate = () => {
      const { w, h, dpr } = sizeRef.current;
      if (w === 0) {
        animFrameRef.current = requestAnimationFrame(animate);
        return;
      }

      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      ctx.clearRect(0, 0, w, h);

      // Screen flash
      if (flashRef.current > 0) {
        ctx.fillStyle = `rgba(180, 200, 255, ${flashRef.current * 0.015})`;
        ctx.fillRect(0, 0, w, h);
        flashRef.current -= 1;
      }

      // Spawn initial burst only once, after 1s delay
      if (!spawnedRef.done && Date.now() - startTime >= 1000) {
        spawnedRef.done = true;
        const count = 3 + Math.floor(Math.random() * 3);
        for (let i = 0; i < count; i++) {
          const bolt = createBolt(w, h);
          boltsRef.current.push(bolt);
        }
      }

      // Collect all bolt segments into batches to minimize state changes
      const glowSegs: Point[][] = [];
      const glowAlphas: number[] = [];
      const coreSegs: Point[][] = [];
      const coreAlphas: number[] = [];
      const coreWidths: number[] = [];

      for (const bolt of boltsRef.current) {
        collectSegments(bolt, 1, glowSegs, glowAlphas, coreSegs, coreAlphas, coreWidths);
      }

      // Pass 1: wide glow — single compositing mode, no shadowBlur
      ctx.lineCap = "round";
      ctx.lineJoin = "round";
      ctx.globalCompositeOperation = "lighter";

      for (let i = 0; i < glowSegs.length; i++) {
        const a = glowAlphas[i];
        ctx.beginPath();
        tracePath(ctx, glowSegs[i]);
        ctx.strokeStyle = `rgba(140, 160, 220, ${a * 0.12})`;
        ctx.lineWidth = 10;
        ctx.stroke();
      }

      // Pass 2: core bolt
      ctx.globalCompositeOperation = "source-over";

      for (let i = 0; i < coreSegs.length; i++) {
        const a = coreAlphas[i];
        const lw = coreWidths[i];
        ctx.beginPath();
        tracePath(ctx, coreSegs[i]);
        ctx.strokeStyle = `rgba(210, 220, 255, ${a * 0.9})`;
        ctx.lineWidth = lw;
        ctx.stroke();
      }

      // Pass 3: white-hot center
      for (let i = 0; i < coreSegs.length; i++) {
        const a = coreAlphas[i];
        const lw = coreWidths[i];
        ctx.beginPath();
        tracePath(ctx, coreSegs[i]);
        ctx.strokeStyle = `rgba(255, 255, 255, ${a * 0.7})`;
        ctx.lineWidth = lw * 0.35;
        ctx.stroke();
      }

      // Decay bolts
      for (let i = boltsRef.current.length - 1; i >= 0; i--) {
        const bolt = boltsRef.current[i];
        bolt.alpha -= bolt.decay;
        if (bolt.alpha <= 0) {
          boltsRef.current.splice(i, 1);
        }
      }

      animFrameRef.current = requestAnimationFrame(animate);
    };

    animFrameRef.current = requestAnimationFrame(animate);

    const handleClick = () => {
      const { w, h } = sizeRef.current;
      if (w === 0) return;
      const count = 2 + Math.floor(Math.random() * 3);
      for (let i = 0; i < count; i++) {
        boltsRef.current.push(createBolt(w, h));
      }
    };

    canvas.addEventListener("click", handleClick);

    return () => {
      cancelAnimationFrame(animFrameRef.current);
      window.removeEventListener("resize", resize);
      canvas.removeEventListener("click", handleClick);
    };
  }, []);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 cursor-pointer"
      style={{ opacity: 0.55 }}
    />
  );
}
