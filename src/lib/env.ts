// "Xfce · X11" for the footer of both programs. Same mapping as Welcome's and
// Repo Selector's sidebar.
export interface EnvInfo {
  desktopEnvironment: string;
  displayProtocol: string;
}

const DE_LABELS: Record<string, string> = {
  gnome: "GNOME",
  kde: "Plasma",
  xfce: "Xfce",
  unknown: "",
};

const PROTO_LABELS: Record<string, string> = {
  wayland: "Wayland",
  x11: "X11",
  unknown: "",
};

export function envLabel(info: EnvInfo | null): string {
  if (!info) return "";
  const desktop = DE_LABELS[info.desktopEnvironment] ?? info.desktopEnvironment;
  const protocol = PROTO_LABELS[info.displayProtocol] ?? "";
  return [desktop, protocol].filter(Boolean).join(" · ");
}

// "3.0.0-1-beta" is shown as "3.0.0-beta": the Debian revision never appears
// in the interface.
export function cleanVersion(version: string): string {
  return version.replace(/-\d+-/, "-");
}
