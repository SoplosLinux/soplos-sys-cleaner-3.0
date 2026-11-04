# Changelog

All notable changes to Soplos Sys Cleaner are documented in this file.

## [3.0.0-9-beta] - 2026-11-04

### Changed

- The button that cleans the selection stays fixed at the bottom of the window in every page, so it is always in reach without scrolling down a long list.
- The manual page describes the Snapshots page and the Disk space page.

### Fixed

- The application metadata points to the launcher that the package installs, so the software centers link the entry of the menu with the program.

## [3.0.0-8-beta] - 2026-10-29

### Changed

- The window no longer has the frame of the system: it is a card with rounded corners and its own minimize, maximize and close buttons at the top right. It can be moved from the top strip, the banner and the side bar, and double click on the strip maximizes it. The rounded corners need a compositing manager running, as the one of Xfce, Plasma or GNOME.

## [3.0.0-7-beta] - 2026-10-28

### Changed

- New application icon, in the sizes of the package, the window and the interface.
- New hero image, with the circuit style of the other Soplos 3.0 programs, for the advanced and the basic program.
- New screenshots of the basic program, the overview, Languages and History, taken with the interface in English.

## [3.0.0-6-beta] - 2026-10-25

### Changed

- Snapshots: the page now reads the list when it is opened, asking for the password once, and no longer waits for the button. If the password is refused or the read fails, the button stays to try again. Reading the list and making a snapshot no longer read it twice, so only one password is asked.

### Fixed

- The Timeshift snapshots in BTRFS mode were all shown as 0 B, because no size was calculated for them. The size of each one is now what it holds that nothing else does, read from the BTRFS quotas (qgroups), which have to be on, as Timeshift also needs for its own sizes. When a size cannot be told, because the quotas are off or the tool gives none, the page shows a dash and not 0 B.

## [3.0.0-5-beta] - 2026-10-24

### Added

- New page, Disk space, in a new "Analysis" group of the sidebar: where the space goes, folder by folder, the largest first, with a bar and the share of each entry, breadcrumbs and a button to go up. It has two scopes, the user's home and the whole system, and shows how much is free on the disk. It only shows: there is no delete button, because what to free is the user's decision, made in the folders. Sizes are the space taken on the disk, a hard-linked file counts once and other filesystems are not added to their parent. The system scope can be read again as administrator (new `disk-list` operation of the privileged helper) to count the folders the user cannot open.

### Changed

- Documentation: every package folder under `/usr/share/doc` that holds more than its copyright file is listed, whatever its size. Before, only the ones over 1 MB were, so apps with small documentation, such as the screenshots of Repo Selector, AppImage Manager and WebApp Manager, never appeared. The user chooses what goes; the copyright files still stay.

## [3.0.0-4-beta] - 2026-10-23

### Added

- New category, Snapshots: the restore points of Timeshift (in its rsync and its BTRFS mode) and of Snapper. Reading them needs the administrator password, so nothing is listed until the page asks for it with a button, and a scan of the window never asks for it. The snapshots are always for review, never preselected and never part of the quick cleanups, and the newest one of each tool (and Snapper configuration) is locked, so there is always a point to go back to. The helper that runs as root accepts a delete only for a snapshot that the tool itself lists at that moment and never for the newest of its set, and, when the boot menu lists snapshots (grub-btrfs) and its daemon is not running, it makes the menu again. The size is shown for rsync snapshots kept on the root device (what only that snapshot holds) and for Snapper when its quota is on; the other cases show none. The page can also make a snapshot now, in Timeshift or in a Snapper configuration, with a short comment.
- `CHANGELOG.md` is installed as `/usr/share/doc/soplos-sys-cleaner/changelog.gz`.
- AppStream metainfo: categories and keywords in the eight languages, the same ones as the desktop entry, and the `3.0.0-beta` release.
- The Debian copyright now declares Lucide (ISC), used for the interface icons.
- README: version, license, platform, Tauri, Rust and Svelte badges.

### Changed

- The sidebar is wider (258 pixels) and only its list scrolls, so the title and the footer stay in view; its orange bar is thin and keeps its own room, and no longer sits on the figures of the rows.
- The window follows the light or dark theme of the desktop. It starts in the theme of the desktop and notices when it changes. The theme is read from the XDG portal and, when that says nothing, from each desktop: the colors of `kdeglobals` on Plasma, the name of the GTK theme on Xfce (a guess from words such as "dark") and the color scheme of GNOME. The button of the sidebar still switches the theme and keeps what it chose; switching to the theme the desktop has goes back to following it.
- Manual page rewritten: both programs, the risk labels, every cleaning page, the privilege model, the options `--check`, `--notify` and `--run-ops`, all keyboard shortcuts and the files of the history, the alerts and the advanced launcher. It is installed compressed, as `/usr/share/man/man1/soplos-sys-cleaner.1.gz`.

### Fixed

- The type errors and the warning that the project had: the journal scanner did not declare its rows, the page of a category took the name of the tab as a plain string, and the About dialog did not use its `mode`.
- AppStream metainfo: the developer and project group identifiers were `org.soploslinux`. They are now `org.soplos`, matching the soplos.org domain and the other Soplos apps.
- AppStream metainfo: the screenshots were bare file names instead of web addresses, so software centers could not show them and the file did not validate. They now use their `raw.githubusercontent.com` address.
- Italian, Portuguese, Romanian and Russian were missing `dashboard.pendingTotal`, `dashboard.actionPreview` and `dashboard.allCategories`.
- The texts of the user cache page (`cacheView.context`, `cacheView.preservedTitle`, `cacheView.preserved` and `cacheView.equivalentTitle`) existed only in Spanish, so the page showed raw keys in the other seven languages, English included. They are now in all eight.

## [3.0.0-3-beta] - 2026-10-19

### Added

- Added Unix manual page documentation (`soplos-sys-cleaner.1`) installed to `/usr/share/man/man1/`.
- Full 8-language translations for desktop entry (`soplos-sys-cleaner.desktop.hbs`) and AppStream metainfo.
- Packaged application screenshots into `/usr/share/doc/soplos-sys-cleaner/screenshots/`.
- Updated Debian packaging metadata and bumped project release to `3.0.0-3-beta`.

## [3.0.0-2-beta] - 2026-10-14

### Added

- Residual configuration page: packages in the dpkg `rc` state (removed but with their configuration files left behind, which is what `apt remove` leaves), with the size of the configuration still on disk. They are purged with `apt-get purge`, always marked Review, and the privileged side asks dpkg again and refuses any package that is not in the `rc` state. Nothing in a home folder is touched.
- The health rings of both programs fill from zero up to their value, with the number counting up, and the weekly bars of the Summary grow from zero. The ring waits for the first scan before filling, so it does not fill for a value that is about to change, and nothing moves when the system asks to reduce motion.
- Orphaned packages page: what `apt autoremove` would remove, with sizes, ported from 2.x. All rows are marked Review, so they never enter the standard or full cleanup. The full list of what would be removed is shown first, and the privileged side asks apt again and refuses any package apt does not itself consider unneeded.
- Installed apps now lists Flatpak, Snap and AppImage apps next to the APT ones, with a Source column. Flatpak and Snap show applications only (runtimes, bases and old revisions have their own pages). AppImages are the ones Soplos AppImage Manager keeps in `~/AppImages`. Everything is marked Review. User Flatpaks and AppImages are removed without a password; the rest goes through the privileged helper, which checks each Snap is an installed application.
- Export reinstall script on the Installed apps page: a `.sh` that installs the APT, Flatpak (from the remote each came from) and Snap apps again after a format, one command at a time so a failure does not stop the rest, with a summary at the end. AppImages are not reinstalled by it; they are listed in the script with their name, version and download address.

### Changed

- The search box has a button (and the Esc key) to clear what was typed, and the bar with the filters, the search box and the summary stays at the top of the page while the list scrolls, with a strip of the page colour above it so no rows show between it and the header.
- The apps export is no longer a plain list of package names: it is the reinstall script described above. Base packages every installation already has (essential, or priority required or important) are left out of it, and APT is called once for all the packages, falling back to one by one only if that fails.
- APT package cache page lists every cached `.deb` as its own row, with its size, instead of one row for the whole folder, so single files can be picked. Cleaning removes only the selected files, validated again as root (plain file names, `.deb` only, regular files only).

### Fixed

- The keyboard shortcuts stopped working after F1 was pressed once, F1 included. The About dialog counts itself as an open dialog so the shortcuts stay quiet while it is shown, but it registered the matching decrement inside an `async` function, whose returned cleanup Svelte ignores, so the count never went back to zero. It is now counted and released in a normal function. The About dialog no longer lists the keyboard shortcuts.
- Purging residual configuration could leave the system without a login screen. A display manager that another one replaced (SDDM, replaced by Plasma Login Manager) stays in the `rc` state, and its purge script can delete the shared `/etc/systemd/system/display-manager.service` link of the one in use, so the next boot reached `graphical.target` with nothing to start and landed on a text console. Display managers, boot loaders and base system packages (a fixed list of names and prefixes such as `grub`, `shim`, `initramfs-tools`, `dracut`, `polkit`, `systemd`, and anything that provides `x-display-manager`) are now shown locked and never purged, and the privileged side refuses them. The purge script of `grub-efi-amd64` also deletes `/etc/default/grub` even when another GRUB package uses it. As a net for any package, the helper notes that link, `/etc/X11/default-display-manager` and `/etc/default/grub` before every batch and puts them back if the batch removed them.
- The user cache cleanup deleted live state. `~/.cache/ibus` holds the socket of the IBus daemon: cleaning it while the session was open left the daemon alive but unreachable, and GTK apps lost the keyboard until the session was restarted. An entry of `~/.cache` is now protected, shown locked with the reason and never deleted, while it holds something alive: the folder of a daemon that is running (`ibus`, `fcitx`, `fcitx5`), a Unix socket somebody is bound to, a `.pid` file of a process that exists, a `.lock` or `LOCK` file a process has open (looked for up to four levels deep), or anything a running program has open. What a program that is no longer running left behind (a dead socket, a stale lock or pid file) is not protected and is cleaned. The delete command checks this again by itself and refuses the whole request if any path is protected or outside `~/.cache`, whatever the interface sends. The alerts add up only what can really be cleaned. Texts in the eight languages.
- Turning the alerts off now clears everything: the rules, the frequency and the notification history go back to empty, so the page looks like it did before the alerts were ever set up and turning them on again means choosing everything again. Before, the values stayed in the form, against the rule that alerts have no defaults. Only the notification texts are kept, because they follow the interface language.
- The storage card of the Summary listed every installed snap (`/snap/core24/...`, always 100% full) as a disk. Loop devices, which are read-only images, are no longer listed.
- The Content column of the Languages page showed internal names (`kde`, `locale`, `man`). It now says Translations, Desktop help, KDE help and Manual pages, in the eight languages. KDE help appears on any desktop where its packages are installed.
- Languages page was empty on systems where `localepurge` has run. It leaves empty folders for the variants it removed (`es_AR`, `en_GB`...); the page counted them as removable but they free nothing, so the whole language, including the protected active language and English, was dropped from the list. Variants that are only empty folders no longer count as removable, so the protected rows are shown.
- Alerts on GNOME: the test notification was dropped by the shell because it was sent from the program's own window. It is now sent by a separate process started through systemd, the same way the timer sends the real alerts. On GNOME the notification no longer carries the `desktop-entry` hint, which made the shell close it at once.
- The basic program's footer shows the desktop, the display protocol and the version (without the Debian revision), like the advanced program, and its shortcuts are listed in the tooltip.
- The program's desktop entry declares `X-GNOME-UsesNotifications`.

## [3.0.0-1-beta] - 2026-10-10

### Added

- Search box and sortable columns on every cleaning page, and an export of the installed apps list (one package per line, for reinstalling after a format) on the Installed apps page.
- Alerts page in the advanced program. The user chooses when to be notified (time since the last cleanup, size ready to clean, or both) and how often to check, then turns the service on. Nothing has a default: until the user turns it on there is no service and no notification. The service is a systemd user timer that only sends a desktop notification (freedesktop D-Bus, so it works on any desktop); it never cleans anything by itself. Turning it off removes the timer.
- Two programs in one package. `soplos-sys-cleaner` is the advanced program; `soplos-sys-cleaner-basic` shows a health ring and a single standard cleanup (the safe part of the full cleanup, with at most one password prompt) for people who do not need the detail. The basic program can open the advanced one and closes itself when it does. Only the basic program has a menu entry, and it can add or remove a user launcher for the advanced one. Only one of the two runs at a time, and both write to the same history.
- Kernels page: old kernels with their headers and source, leftover directories in `/lib/modules` and `/usr/src`, and build-tool packages no installed kernel uses. The running kernel and the newest other kernel are always protected, and kernels installed by hand are never treated as leftovers.
- Drivers page: packages for hardware this machine does not have (graphics stacks, printer drivers, hypervisor guest tools, KVM host packages) and orphaned DKMS builds. Detection combines the dracut configuration, PCI and USB devices, loaded modules bound to a device and the kernel log, plus virtualisation detection.
- Firmware page: firmware families under `/lib/firmware` that no detection layer claims. Families owned by a package that would bring them back on upgrade are marked as such, and a family stays protected while its driver is installed.
- Languages page: one row per language with its regional variants grouped, covering translations, desktop and KDE help and translated manual pages. The active language is protected by exact code, together with the codes gettext falls back to, and English is always kept.
- Documentation page: large per-package documentation folders under `/usr/share/doc` (copyright files are always kept) plus Info, Qt, GTK-Doc and Devhelp documentation.
- Dry run before removing packages (`apt-get -s`): the full list of what would be removed is shown first, and the cleanup is blocked when it would also remove something the system needs.
- Journal retention by time or by size, with an approximate estimate of how much each setting frees.
- System card on the Overview page: distribution, kernel, session, uptime, CPU (usage and temperature), memory, swap and GPU, refreshed every 3 seconds.
- History receipts: kind of cleanup (quick, full or manual), duration, command, items and result, with expandable rows, a filter by kind, failed cleanups, up to 1000 entries and CSV export.
- Risk labels (Safe, Review, Protected). Items marked Review are never preselected and never part of the quick or full cleanup.
- Keyboard shortcuts: F1 (About), Ctrl+Q (quit), Ctrl+Tab and Ctrl+Shift+Tab (sections, advanced program), F5 or Ctrl+R (rescan), Ctrl+A (select all or none), Ctrl+Shift+A (only Safe items), Ctrl+Enter (clean selection), Ctrl+I (notes) and Ctrl+E (export the history). The About dialog lists them.
- README, CHANGELOG and `package.sh`.

### Changed

- The interface follows the new layout: an Overview page, one table template shared by every cleaning page, and the hero banner on every page. The explanatory panels start hidden and are shown with the Notes button.
- Every operation that needs privileges runs in a single `pkexec` call per batch, so one password covers a whole cleanup, and the initramfs is rebuilt once at the end of the batch.
- A cleanup is recorded in the history only when its command succeeded, and with the bytes really freed. For the journal that is the measured difference, not the size of the journal.
- The package now depends on `pciutils`, `usbutils`, `kmod`, `apt`, `dpkg`, `systemd` and `dracut` or `initramfs-tools`.
- The version shown in the About dialog and in the sidebar footer never includes the Debian revision.

### Fixed

- Unused Flatpak runtimes installed for the user are now removed without privileges. Before, the administrator could not see them and the cleanup failed. Only system-wide ones use the password prompt.
- Temporary files lists everything in `/tmp` and `/var/tmp`, as 2.x did. Entries changed in the last day are marked Review, because they may still be in use, and are left out of the standard cleanup.
- The privileged helper no longer runs shell scripts as root. It receives named operations and validates each one again as root before running it with a fixed argument list. Deleting from `/tmp` and `/var/tmp` is limited to direct children, refuses symbolic links and never accepts the directories themselves. This closes the command-injection risk through file names in `/tmp` and the "run any script as root" capability of the polkit action.
- If the hardware detection tools are missing, firmware and drivers are shown as protected instead of unused, and the privileged helper refuses to remove them.
- Typing in the window is no longer lost when IBus is selected as the input method but its daemon cannot be reached (common in virtual machines). The program now falls back to GTK's built-in input method in that case, and leaves IBus alone when it works.
- The polkit message now describes what the action does.
- The recorded recovered space no longer includes cancelled or failed cleanups, and translation keys that were shown raw in the interface were added in all eight languages.

## [3.0.0-beta] - 2026-10-10

First version of Soplos Sys Cleaner on Tauri 2, with a Rust backend and a Svelte frontend, replacing the Python 3 and GTK3 application.
