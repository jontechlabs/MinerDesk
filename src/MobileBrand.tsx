import { useId } from "react";

/** MinerDesk Mobile's native connected-M mark; mirrored in docs/brand/mobile. */
export default function MobileBrand({ className, size = 52, title = "MinerDesk Mobile" }: {
  className?: string;
  size?: number;
  title?: string;
}) {
  const gradientId = `mobile-brand-${useId().replace(/:/g, "")}`;
  const titleId = `${gradientId}-title`;
  return <svg className={className} width={size} height={size} viewBox="0 0 1024 1024" role="img" aria-labelledby={titleId} focusable="false">
    <title id={titleId}>{title}</title>
    <defs><linearGradient id={gradientId} x1="0" y1="0" x2="1" y2="0"><stop stopColor="#70f6a0" /><stop offset="1" stopColor="#48d5f7" /></linearGradient></defs>
    <rect width="1024" height="1024" rx="235" fill="#0c1423" />
    <path fill={`url(#${gradientId})`} fillRule="evenodd" d="M302 692 L302 360 Q302 320 339 320 Q354 320 365 333 L512 485 L659 333 Q670 320 685 320 Q722 320 722 360 L722 692 L638 692 L638 466 L542 564 Q512 594 482 564 L386 466 L386 692 Z M333 650a11 11 0 1 0 22 0a11 11 0 1 0 -22 0Z M501 537a11 11 0 1 0 22 0a11 11 0 1 0 -22 0Z M669 650a11 11 0 1 0 22 0a11 11 0 1 0 -22 0Z" />
  </svg>;
}
