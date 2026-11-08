import type { SVGProps } from "react";

type IconProps = SVGProps<SVGSVGElement> & { size?: number };

function Svg({ size = 16, children, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...rest}
    >
      {children}
    </svg>
  );
}

export function IconImport(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 3v12" />
      <path d="m7.5 10.5 4.5 4.5 4.5-4.5" />
      <path d="M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3" />
    </Svg>
  );
}

export function IconExport(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 16V4" />
      <path d="m7.5 8.5 4.5-4.5 4.5 4.5" />
      <path d="M4 16v3a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-3" />
    </Svg>
  );
}

export function IconUndo(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M8 5 4 9l4 4" />
      <path d="M4 9h9a7 7 0 0 1 0 14h-2" />
    </Svg>
  );
}

export function IconRedo(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m16 5 4 4-4 4" />
      <path d="M20 9h-9a7 7 0 0 0 0 14h2" />
    </Svg>
  );
}

export function IconScissors(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="6.5" cy="6.5" r="2.5" />
      <circle cx="6.5" cy="17.5" r="2.5" />
      <path d="M8.7 8.2 20 18" />
      <path d="M8.7 15.8 20 6" />
    </Svg>
  );
}

export function IconAuto(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 20 13 4l3 7 4-2-4 11Z" />
      <path d="M9 12h4" />
      <path d="M17 17.5v3" />
      <path d="M15.5 19h3" />
    </Svg>
  );
}

export function IconSliders(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 7h10" />
      <path d="M18 7h2" />
      <circle cx="16" cy="7" r="2" />
      <path d="M4 17h4" />
      <path d="M12 17h8" />
      <circle cx="10" cy="17" r="2" />
      <path d="M4 12h6" />
      <path d="M14 12h6" />
      <circle cx="12" cy="12" r="2" />
    </Svg>
  );
}

export function IconTrash(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 7h16" />
      <path d="M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2" />
      <path d="m6 7 1 12a2 2 0 0 0 2 2h6a2 2 0 0 0 2-2l1-12" />
      <path d="M10 11v6" />
      <path d="M14 11v6" />
    </Svg>
  );
}

export function IconCopy(props: IconProps) {
  return (
    <Svg {...props}>
      <rect x="9" y="9" width="11" height="11" rx="2" />
      <path d="M15 9V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h3" />
    </Svg>
  );
}

export function IconEye(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12Z" />
      <circle cx="12" cy="12" r="3" />
    </Svg>
  );
}

export function IconEyeOff(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 4l16 16" />
      <path d="M9.6 6.2A9.7 9.7 0 0 1 12 6c6 0 9.5 6 9.5 6a16 16 0 0 1-2.4 3.2" />
      <path d="M6.6 8.1A15.4 15.4 0 0 0 2.5 12S6 18 12 18a9.5 9.5 0 0 0 3.2-.55" />
      <path d="M9.9 9.9a3 3 0 0 0 4.2 4.2" />
    </Svg>
  );
}

export function IconPlus(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 5v14" />
      <path d="M5 12h14" />
    </Svg>
  );
}

export function IconCheck(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m5 12.5 4.5 4.5L19 7" />
    </Svg>
  );
}

export function IconAlert(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M12 4 21 20H3Z" />
      <path d="M12 9v5" />
      <path d="M12 17.2v.1" />
    </Svg>
  );
}

export function IconInfo(props: IconProps) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5" />
      <path d="M12 7.8v.1" />
    </Svg>
  );
}

export function IconClose(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="m6 6 12 12" />
      <path d="M18 6 6 18" />
    </Svg>
  );
}

export function IconFit(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M4 9V4h5" />
      <path d="M20 9V4h-5" />
      <path d="M4 15v5h5" />
      <path d="M20 15v5h-5" />
      <rect x="8" y="8" width="8" height="8" rx="1" />
    </Svg>
  );
}

export function IconArrow(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M5 12h14" />
      <path d="m13 6 6 6-6 6" />
    </Svg>
  );
}

export function IconFolder(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2Z" />
    </Svg>
  );
}

export function IconWarning(props: IconProps) {
  return <IconAlert {...props} />;
}

export function IconRefresh(props: IconProps) {
  return (
    <Svg {...props}>
      <path d="M20 11a8 8 0 0 0-14.3-4.9L4 8" />
      <path d="M4 4v4h4" />
      <path d="M4 13a8 8 0 0 0 14.3 4.9L20 16" />
      <path d="M20 20v-4h-4" />
    </Svg>
  );
}

export function BrandMark({ size = 38 }: { size?: number }) {
  return (
    <svg
      className="brand-mark"
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      aria-hidden="true"
    >
      <defs>
        <radialGradient id="bmBg" cx="0.5" cy="0.3" r="0.9">
          <stop offset="0" stopColor="#1b2530" />
          <stop offset="0.55" stopColor="#0d1218" />
          <stop offset="1" stopColor="#06080b" />
        </radialGradient>
        <radialGradient id="bmHalo" cx="0.5" cy="0.5" r="0.5">
          <stop offset="0" stopColor="#3ecfb2" stopOpacity="0.22" />
          <stop offset="0.6" stopColor="#3ecfb2" stopOpacity="0.05" />
          <stop offset="1" stopColor="#3ecfb2" stopOpacity="0" />
        </radialGradient>
        <linearGradient id="bmRim" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ffffff" stopOpacity="0.16" />
          <stop offset="0.5" stopColor="#ffffff" stopOpacity="0.04" />
          <stop offset="1" stopColor="#ffffff" stopOpacity="0.10" />
        </linearGradient>
        <linearGradient id="bmTop" x1="0.1" y1="0" x2="0.55" y2="1">
          <stop offset="0" stopColor="#7cf3d8" />
          <stop offset="1" stopColor="#2aa88f" />
        </linearGradient>
        <linearGradient id="bmRight" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#42d4b6" />
          <stop offset="1" stopColor="#137563" />
        </linearGradient>
        <linearGradient id="bmLeft" x1="0" y1="0" x2="0.2" y2="1">
          <stop offset="0" stopColor="#1e6259" />
          <stop offset="1" stopColor="#0a2321" />
        </linearGradient>
        <linearGradient id="bmCut" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#ffe1a0" />
          <stop offset="0.45" stopColor="#ffb224" />
          <stop offset="1" stopColor="#ff8c00" />
        </linearGradient>
        <linearGradient id="bmEdge" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#8af7de" stopOpacity="0.85" />
          <stop offset="1" stopColor="#1d7d6c" stopOpacity="0.35" />
        </linearGradient>
        <filter id="bmGlow" x="-60%" y="-60%" width="220%" height="220%">
          <feGaussianBlur stdDeviation="14" />
        </filter>
      </defs>
      <rect width="1024" height="1024" rx="232" fill="url(#bmBg)" />
      <rect
        x="8"
        y="8"
        width="1008"
        height="1008"
        rx="224"
        fill="none"
        stroke="url(#bmRim)"
        strokeWidth="3"
      />
      <circle cx="512" cy="500" r="360" fill="url(#bmHalo)" />
      <g filter="url(#bmGlow)" opacity="0.55">
        <path
          d="M749.46 379.66 L300.34 682.59 L274.54 644.34 L723.66 341.41 Z"
          fill="#ffb224"
        />
      </g>
      <path
        d="M749.46 379.66 L761.64 386.68 L761.64 675.58 L524.90 812.00 L300.34 682.59 L274.54 644.34 L262.36 637.32 L262.36 348.42 L499.10 212.00 L723.66 341.41 Z"
        fill="none"
        stroke="url(#bmEdge)"
        strokeWidth="7"
        strokeLinejoin="round"
      />
      <path d="M749.46 379.66 L761.64 386.68 L524.90 531.13 Z" fill="url(#bmTop)" />
      <path
        d="M499.10 212.00 L723.66 341.41 L499.10 492.87 L262.36 348.42 Z"
        fill="url(#bmTop)"
      />
      <path
        d="M524.90 531.13 L761.64 386.68 L761.64 675.58 L524.90 812.00 Z"
        fill="url(#bmRight)"
      />
      <path d="M524.90 531.13 L524.90 812.00 L300.34 682.59 Z" fill="url(#bmLeft)" />
      <path
        d="M499.10 492.87 L274.54 644.34 L262.36 637.32 L262.36 348.42 Z"
        fill="url(#bmLeft)"
      />
      <path
        d="M749.46 379.66 L300.34 682.59 L274.54 644.34 L723.66 341.41 Z"
        fill="url(#bmCut)"
      />
      <path
        d="M749.46 379.66L300.34 682.59"
        stroke="#fff2cd"
        strokeWidth="3.5"
        strokeLinecap="round"
        opacity="0.7"
      />
    </svg>
  );
}
