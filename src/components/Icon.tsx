interface IconProps {
  name: "add" | "blade" | "check" | "chevron" | "collapse" | "export" | "file" | "folder" | "import" | "pause" | "play" | "record" | "remove" | "restore" | "return" | "select" | "settings" | "volume" | "warning";
  size?: number;
}

export default function Icon({ name, size = 18 }: IconProps) {
  const common = {
    fill: "none",
    stroke: "currentColor",
    strokeLinecap: "round" as const,
    strokeLinejoin: "round" as const,
    strokeWidth: 1.7,
  };
  const paths: Record<IconProps["name"], React.ReactNode> = {
    add: <><path d="M12 5v14M5 12h14" /></>,
    blade: <><path d="m6 4 11 16M16 4 5 20" /><path d="M4 12h16" /></>,
    check: <path d="m5 12 4 4L19 6" />,
    chevron: <path d="m9 6 6 6-6 6" />,
    collapse: <><path d="M4 8h6V2M20 16h-6v6" /><path d="m10 8-7-7M14 16l7 7" /></>,
    export: <><path d="M12 3v12M7 10l5 5 5-5" /><path d="M5 20h14" /></>,
    file: <><path d="M6 3h8l4 4v14H6z" /><path d="M14 3v5h5" /></>,
    folder: <path d="M3 6h7l2 2h9v11H3z" />,
    import: <><path d="M12 21V9M7 14l5-5 5 5" /><path d="M5 4h14" /></>,
    pause: <><path d="M8 6v12M16 6v12" /></>,
    play: <path d="m8 5 11 7-11 7z" />,
    record: <circle cx="12" cy="12" r="6" fill="currentColor" stroke="none" />,
    remove: <><path d="M5 7h14M9 7V4h6v3M8 7l1 14h6l1-14" /></>,
    restore: <><path d="M4 7v6h6" /><path d="M5 13a8 8 0 1 0 2-6" /></>,
    return: <><path d="M5 5v6h6" /><path d="M5 11c2-5 10-7 14-2 3 4 1 9-3 11" /></>,
    select: <path d="M5 3v17l5-5 4 7 3-2-4-7h7z" />,
    settings: <><circle cx="12" cy="12" r="3" /><path d="M12 2v3M12 19v3M2 12h3M19 12h3M5 5l2 2M17 17l2 2M19 5l-2 2M7 17l-2 2" /></>,
    volume: <><path d="M3 14h4l5 5V5L7 10H3z" /><path d="M16 9c2 2 2 4 0 6M19 6c4 4 4 8 0 12" /></>,
    warning: <><path d="M12 3 2 21h20z" /><path d="M12 9v5M12 18h.01" /></>,
  };

  return <svg className="icon" width={size} height={size} viewBox="0 0 24 24" aria-hidden="true" {...common}>{paths[name]}</svg>;
}
