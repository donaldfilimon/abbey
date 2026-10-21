const paths: Record<string, string> = {
  chat: "M4 4h16v12H9l-5 4V4Z",
  runs: "m8 5 11 7-11 7V5Z",
  automations: "m13 2-9 12h7l-1 8 10-13h-7l0-7Z",
  memory: "M5 4h14v16H5V4Zm4 4h6m-6 4h6m-6 4h3",
  models: "m12 3 9 5-9 5-9-5 9-5Zm-9 9 9 5 9-5M3 16l9 5 9-5",
  tools: "m14 6 4 4m-9 2-6 6 3 3 6-6m-1-9a6 6 0 0 0 7 7l3-3-4-1-1-4-3 1Z",
  doctor:
    "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8Zm0-5v2m0 14v2M3 12h2m14 0h2M5 5l2 2m10 10 2 2M5 19l2-2M17 7l2-2",
  routes:
    "M5 4v12a4 4 0 0 0 4 4h10M5 9h10a4 4 0 0 0 4-4M3 4h4m10 0h4m-4 14 2 2-2 2",
  claims: "m12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Zm-4 9 3 3 5-6",
  training: "m3 9 9-5 9 5-9 5-9-5Zm4 3v6l5 3 5-3v-6",
  cluster: "M9 3h6v6H9V3ZM3 15h6v6H3v-6Zm12 0h6v6h-6v-6ZM12 9v3m-6 3v-3h12v3",
  search: "M10 3a7 7 0 1 0 0 14 7 7 0 0 0 0-14Zm5 12 6 6",
  panel: "M3 4h18v16H3V4Zm6 0v16",
  plus: "M12 5v14M5 12h14",
  arrow: "M12 19V5m-6 6 6-6 6 6",
  spark: "m12 3 3 6 6 3-6 3-3 6-3-6-6-3 6-3 3-6Z",
  chevron: "m9 5 7 7-7 7",
  refresh: "M20 11a8 8 0 1 0-2 7M20 4v7h-7",
};
export function Icon({
  name,
  className = "",
}: {
  name: string;
  className?: string;
}) {
  return (
    <svg
      className={`icon ${className}`}
      width="20"
      height="20"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.6"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d={paths[name] ?? paths.spark} />
    </svg>
  );
}
