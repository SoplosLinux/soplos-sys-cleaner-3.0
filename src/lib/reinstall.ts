import type { Row } from "./categories";

// The script that installs the listed apps again after a format. APT, Flatpak
// and Snap apps are reinstalled by command. AppImages are only listed, with
// the address they came from, so each one can be downloaded again by hand.
// Every command goes through run(), so one that fails (a package that no
// longer exists, no network) is noted and the rest still run.

// Single quotes around anything that is not a plain word, so a name can never
// be read as shell syntax.
function quote(value: string): string {
  return /^[A-Za-z0-9._+:/@=-]+$/.test(value) ? value : `'${value.replace(/'/g, `'\\''`)}'`;
}

// One line of a comment: no line breaks.
function oneLine(value: string): string {
  return value.replace(/[\r\n]+/g, " ").trim();
}

export function buildReinstallScript(rows: Row[], when: Date): string {
  const of = (kind: string) => rows.filter((r) => r.payload?.kind === kind);
  // Base packages (essential, required or important) come with every
  // installation, so they are left out.
  const apt = of("apt")
    .filter((r) => r.payload!.base !== "1")
    .map((r) => r.payload!.name)
    .sort((a, b) => a.localeCompare(b));
  const flatpak = of("flatpak");
  const snap = of("snap");
  const appimages = of("appimage");

  const out: string[] = [
    "#!/usr/bin/env bash",
    `# Reinstall script made by Soplos Sys Cleaner on ${when.toISOString().slice(0, 10)}.`,
    "# Read it before you run it. To run it: bash reinstall-apps.sh",
    "# Lines that start with # are notes. A command that fails is noted and the rest still run;",
    "# the ones that failed are listed at the end.",
    "",
    "failed=()",
    'run() { echo "+ $*"; "$@" || failed+=("$*"); }',
    "",
  ];

  if (apt.length > 0) {
    // One apt call for all of them (fast, and apt sorts out the dependencies).
    // If a package cannot be installed with the rest, apt installs nothing,
    // so they are tried one by one and only the real failures are noted.
    const lines: string[] = [];
    for (let i = 0; i < apt.length; i += 8) lines.push("  " + apt.slice(i, i + 8).map(quote).join(" "));
    out.push(
      "# --- APT ---",
      "# Base packages that every installation already has are not listed.",
      "run sudo apt update",
      "packages=(",
      ...lines,
      ")",
      'echo "+ sudo apt install -y (${#packages[@]} packages)"',
      'if ! sudo apt install -y "${packages[@]}"; then',
      '  echo "A package could not be installed with the rest. Trying them one by one."',
      '  for pkg in "${packages[@]}"; do',
      '    run sudo apt install -y "$pkg"',
      "  done",
      "fi",
      "",
    );
  }

  if (flatpak.length > 0) {
    out.push("# --- Flatpak ---", "# Each app is installed from the remote it came from. That remote must exist on the new system.");
    const flathub = "https://dl.flathub.org/repo/flathub.flatpakrepo";
    if (flatpak.some((r) => r.payload!.origin === "flathub" && r.payload!.installation === "user")) {
      out.push(`run flatpak remote-add --if-not-exists --user flathub ${flathub}`);
    }
    if (flatpak.some((r) => r.payload!.origin === "flathub" && r.payload!.installation !== "user")) {
      out.push(`run sudo flatpak remote-add --if-not-exists --system flathub ${flathub}`);
    }
    for (const r of flatpak) {
      const id = r.payload!.reference.split("/")[0];
      const scope = r.payload!.installation === "user" ? "--user" : "--system";
      const sudo = scope === "--system" ? "sudo " : "";
      out.push(`run ${sudo}flatpak install ${scope} -y ${quote(r.payload!.origin)} ${quote(id)}`);
    }
    out.push("");
  }

  if (snap.length > 0) {
    out.push("# --- Snap ---");
    for (const r of snap) {
      const classic = r.payload!.classic === "1" ? " --classic" : "";
      out.push(`run sudo snap install ${quote(r.payload!.name)}${classic}`);
    }
    out.push("");
  }

  if (appimages.length > 0) {
    out.push(
      "# --- AppImage ---",
      "# Not reinstalled by this script. Download each one again from the address below",
      "# and add it with Soplos AppImage Manager.",
    );
    for (const r of appimages) {
      const p = r.payload!;
      const version = p.version ? ` ${oneLine(p.version)}` : "";
      const where = p.url ? oneLine(p.url) : "no download address known";
      out.push(`#   ${oneLine(p.name)}${version} (${oneLine(p.file)}): ${where}`);
    }
    out.push("");
  }

  out.push(
    'if [ "${#failed[@]}" -gt 0 ]; then',
    "  echo",
    '  echo "These commands failed:"',
    '  printf "  %s\\n" "${failed[@]}"',
    "  exit 1",
    "fi",
    'echo "Done."',
    "",
  );
  return out.join("\n");
}
