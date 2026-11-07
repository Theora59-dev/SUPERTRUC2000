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

export function BrandMark({ size = 34 }: { size?: number }) {
  return (
    <svg
      className="brand-mark"
      width={size}
      height={size}
      viewBox="0 0 1024 1024"
      aria-hidden="true"
    >
      <defs>
        <linearGradient id="stk-face" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#3ecfb2" />
          <stop offset="1" stopColor="#1d7d6c" />
        </linearGradient>
        <linearGradient id="stk-cut" x1="0" y1="0" x2="1" y2="0">
          <stop offset="0" stopColor="#ffb224" stopOpacity="0" />
          <stop offset="0.5" stopColor="#ffb224" />
          <stop offset="1" stopColor="#ffb224" stopOpacity="0" />
        </linearGradient>
      </defs>
      <path
        d="M512 190 806 360 806 690 512 860 218 690 218 360Z"
        fill="none"
        stroke="#3ecfb2"
        strokeWidth="30"
        strokeLinejoin="round"
      />
      <path d="M512 190 806 360 512 530 218 360Z" fill="#1b2a2c" />
      <path d="M512 530 806 360 806 690 512 860Z" fill="url(#stk-face)" opacity="0.4" />
      <path d="M512 530 218 360 218 690 512 860Z" fill="#0e1518" opacity="0.8" />
      <path
        d="M512 190v340M218 360l294 170 294-170M512 530v330"
        fill="none"
        stroke="#3ecfb2"
        strokeWidth="18"
        opacity="0.8"
      />
      <path d="M180 620 844 480" stroke="url(#stk-cut)" strokeWidth="42" strokeLinecap="round" />
      <path
        d="M180 620 844 480"
        stroke="#ffe4a8"
        strokeWidth="10"
        strokeLinecap="round"
        opacity="0.95"
      />
      <circle cx="512" cy="550" r="22" fill="#ffb224" />
    </svg>
  );
}
