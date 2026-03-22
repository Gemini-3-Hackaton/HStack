import type { ComponentType, InputHTMLAttributes, ReactNode } from "react";
import { clsx, type ClassValue } from "clsx";
import { HardDrive, Server } from "lucide-react";
import { twMerge } from "tailwind-merge";
import type { TranslationKey } from "../../i18n";
import { WebGLGrain } from "../WebGLGrain";

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export const THEMES = {
  default: {
    c1: [48, 48, 48],
    c2: [34, 34, 34],
    c3: [24, 24, 24],
    c4: [20, 20, 20],
  },
  emerald: {
    c1: [42, 52, 48],
    c2: [32, 38, 35],
    c3: [24, 26, 25],
    c4: [20, 20, 20],
  },
};

type ShaderColors = (typeof THEMES)[keyof typeof THEMES];

const HStackMark = ({ size = 18 }: { size?: number }) => (
  <svg width={size} height={size} viewBox="0 0 210 210" fill="none" aria-hidden="true">
    <rect x="0" y="0" width="60" height="210" fill="currentColor" />
    <rect x="150" y="0" width="60" height="210" fill="currentColor" />
    <rect x="50" y="45" width="100" height="30" fill="currentColor" />
    <rect x="50" y="90" width="100" height="30" fill="currentColor" />
    <rect x="50" y="135" width="100" height="30" fill="currentColor" />
  </svg>
);

export const PhysicalWrapper = ({
  children,
  outerClass = "",
  innerClass = "",
  checked = false,
  shaderColors = THEMES.default,
}: {
  children: ReactNode;
  outerClass?: string;
  innerClass?: string;
  checked?: boolean;
  shaderColors?: ShaderColors;
}) => (
  <div
    className={cn(
      "relative rounded-[1.25rem] bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)] transition-all duration-300",
      checked ? "opacity-50" : "opacity-100",
      outerClass,
    )}
  >
    <div
      className={cn(
        "relative h-full w-full overflow-hidden rounded-[15px] shadow-[0_2px_5px_rgba(0,0,0,0.7)]",
        innerClass,
      )}
    >
      <WebGLGrain colors={shaderColors} />
      <div className="absolute left-0 right-0 top-0 z-10 h-[1px] bg-white/[0.03]" />
      <div className="absolute bottom-0 left-0 top-0 z-10 w-[1px] bg-white/[0.03]" />
      <div className="relative z-20 h-full w-full">{children}</div>
    </div>
  </div>
);

export const InsetSurface = ({ children, className = "" }: { children: ReactNode; className?: string }) => (
  <div className="rounded-[1.25rem] bg-[#141414] p-[4px] shadow-[inset_0_2px_5px_rgba(0,0,0,0.8)]">
    <div
      className={cn(
        "relative overflow-hidden rounded-[15px] bg-[#121212] shadow-[0_2px_5px_rgba(0,0,0,0.7)]",
        className,
      )}
    >
      <div className="absolute inset-0 bg-[linear-gradient(180deg,rgba(255,255,255,0.03)_0%,rgba(255,255,255,0.01)_18%,rgba(0,0,0,0)_44%,rgba(0,0,0,0.18)_100%)]" />
      <div className="absolute left-0 right-0 top-0 z-10 h-[1px] bg-white/[0.03]" />
      <div className="absolute bottom-0 left-0 top-0 z-10 w-[1px] bg-white/[0.03]" />
      <div className="relative z-20 h-full w-full">{children}</div>
    </div>
  </div>
);

export const EngravedInput = ({
  label,
  className,
  ...props
}: InputHTMLAttributes<HTMLInputElement> & { label: string }) => (
  <div className="flex flex-col gap-2">
    <label className="px-1 text-[9px] font-bold uppercase tracking-widest text-[#777]">{label}</label>
    <InsetSurface>
      <input
        {...props}
        className={cn(
          "relative z-20 w-full bg-transparent px-4 py-3 text-[14px] text-[#D1D1D1] outline-none transition-colors placeholder:text-[#555]",
          className,
        )}
      />
    </InsetSurface>
  </div>
);

export const HOSTING_OPTIONS: Array<{
  mode: 'LocalOnly' | 'CloudOfficial' | 'CloudCustom';
  titleKey: TranslationKey;
  descriptionKey: TranslationKey;
  icon: ComponentType<{ size?: number }>;
}> = [
  {
    mode: 'LocalOnly',
    titleKey: 'hostingLocalTitle',
    descriptionKey: 'hostingLocalDescription',
    icon: HardDrive,
  },
  {
    mode: 'CloudOfficial',
    titleKey: 'hostingOfficialTitle',
    descriptionKey: 'hostingOfficialDescription',
    icon: HStackMark,
  },
  {
    mode: 'CloudCustom',
    titleKey: 'hostingCustomTitle',
    descriptionKey: 'hostingCustomDescription',
    icon: Server,
  },
];