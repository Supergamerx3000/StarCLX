# StarCLX

**English** · [Deutsch](README.de.md)

StarCLX is a free desktop client for STARFACE phone systems, with Linux as its main platform.
It speaks the same interfaces as the STARFACE App for Windows (OneHub gRPC, SIP/TLS with SRTP,
XMPP chat) and works with STARFACE 10 and later.

Not affiliated with STARFACE GmbH. "STARFACE" is used for descriptive purposes only.

## Features

- **Sign in** through the browser (OAuth2 with PKCE); the session is kept in the keyring.
  Self-signed certificates of on-premises systems can be confirmed once
- **Softphone** (SIP/TLS, SRTP) with Call Manager: answer, hold, mute, keypad, consultation,
  transfer, conference, redirect, Call2Go
- **Desk phone control** (CTI): calls are placed through the primary phone, e.g. the
  desk phone; calls on it show up in the Call Manager, even without the softphone
- **Search and address book**: search-as-you-type across all address books; create, edit and
  delete contacts; save unknown numbers from the call list
- **Call list** with filters, notes, "called back" and notifications for missed calls;
  forward a call with a note to colleagues via chat or email
- **Chat** with colleagues (presence, history, files, away on inactivity), group chats
  for the system's groups and ad-hoc group chats like in the STARFACE app
- **Voicemail**: listen to and manage messages
- **Scheduled conferences**: create, edit, delete and start them (date, recurrence, participants)
- **Door intercoms**: camera image in the Call Manager as soon as the door calls, with "Open door";
  additional cameras as tiles (RTSP, Motion JPEG, still image)
- **Function keys** (BLF, speed dial, pickup, park, groups, do not disturb …): view, use and edit
- **Reachability**: redirects, iFMC, displayed caller number
- **Workspace** as tabs or freely arranged tiles
- **Headset buttons** on Jabra, Poly and EPOS (USB HID telephony): answer, hang up,
  mute; the headset rings and shows call and mute state
- **Settings**: audio devices, ringtones, Busylight (Kuando), appearance,
  language (Deutsch, English, Français, Italiano)
- **Desktop integration**: tray icon, autostart, quick dial window,
  keyboard shortcuts (dial selected number, answer, hang up; GNOME, KDE Plasma and other
  desktops with the GlobalShortcuts portal), `tel:`/`callto:`/`sip:` links,
  open a URL or run a program on incoming calls

What changed since the last version is listed in [CHANGELOG.md](CHANGELOG.md) (in German).

## Installation

### Package repository (recommended, with automatic updates)

Debian, Ubuntu and derivatives:

```sh
curl -fsSL https://crazmoe.github.io/StarCLX/starclx.gpg | sudo tee /usr/share/keyrings/starclx.gpg >/dev/null
echo "deb [signed-by=/usr/share/keyrings/starclx.gpg] https://crazmoe.github.io/StarCLX/deb stable main" \
  | sudo tee /etc/apt/sources.list.d/starclx.list
sudo apt update && sudo apt install starclx
```

Fedora (and other RPM distributions with dnf):

```sh
sudo curl -fsSL -o /etc/yum.repos.d/starclx.repo https://crazmoe.github.io/StarCLX/starclx.repo
sudo dnf install starclx
```

Flatpak (pulls in the GNOME runtime from Flathub):

```sh
flatpak install --user https://crazmoe.github.io/StarCLX/flatpak/starclx.flatpakref
```

The repositories are signed; new versions arrive with the regular system update or
`flatpak update`.

### Individual packages

Ready-made packages are also available under [Releases](../../releases): `.deb`, `.rpm`, AppImage,
`.dmg` (macOS) and an installer (Windows).

```sh
sudo apt install ./starclx_<version>_amd64.deb
sudo dnf install ./starclx-<version>-1.x86_64.rpm
```

Or, without installing, make the AppImage executable and run it. The package replaces older builds
named `starface-linuxclient` and registers the link handlers `starface-app://`
(return from the browser login) as well as `tel:`, `callto:` and `sip:`.

Development builds: every build on `main` stores the artifact `starclx-<version>-linux-x86_64`
under **Actions** (version `1.0.0+<run number>`).

### Flatpak

Easiest via the package repository (see above); updates then arrive with `flatpak update`.
Every build also stores the artifact `starclx-<version>-flatpak`, and releases contain
`starclx_<version>_x86_64.flatpak`. To install it on its own (pulls in the GNOME runtime from Flathub):

```sh
flatpak install --user ./starclx_<version>_x86_64.flatpak
flatpak run ch.crazmoe.StarCLX
```

Build it yourself from a `.deb` (without an argument, the latest release is used):
`packaging/flatpak/build.sh [starclx_<version>_amd64.deb]`.

The Flatpak has the same features as the `.deb` and shares settings and confirmed certificates
with it. To do so it is allowed to start programs on the host (program on incoming call,
GNOME keyboard shortcuts, `tel:` links as default), so the sandbox protects little more than the `.deb`.
Differences:

- **Autostart** goes through the desktop environment; GNOME asks once when you turn it on.
- **Disabling `tel:` links** is not possible in StarCLX; choose another program in the
  system settings under default applications.
- **Busylight** and **headset buttons** need the udev rules on the host, once:
  `flatpak run --command=cat ch.crazmoe.StarCLX /app/share/starclx/60-starclx-busylight.rules | sudo tee /etc/udev/rules.d/60-starclx-busylight.rules`
  and the same with `60-starclx-headset.rules`
- **Door cameras with RTSP** use the host's `ffmpeg` (see below), so it must be
  installed there.
- A company CA that is only installed on the host may be missing in the Flatpak;
  self-signed certificates of on-premises systems are confirmed in StarCLX as usual.
- When installed as a single file, updates do not arrive automatically but with the next
  `.flatpak`; via the package repository they come with `flatpak update`.

### Flathub

`packaging/flathub` is the variant for Flathub (app ID `io.github.crazmoe.StarCLX`). It is
built offline from source and has fewer permissions than the project's own Flatpak: starting
programs on incoming calls and registering GNOME keyboard shortcuts directly are not possible
there (URLs on incoming calls, autostart and `tel:` links are; keyboard shortcuts via the portal
work on KDE Plasma and GNOME 48 and later). CI does not build it; it can be built with
`packaging/flathub/prepare.sh` and `flatpak-builder` (see the manifest).

Currently not submitted: Flathub does not accept manifests created with AI and
requires a longer project history ([requirements](https://docs.flathub.org/docs/for-app-authors/requirements)).

Submitting (one time):

1. Create a release tag and run `packaging/flathub/prepare.sh --submission v<version>`.
2. Fork `github.com/flathub/flathub`, create a branch from the `new-pr` branch, add the contents of
   `submission/` and run `git submodule add https://github.com/flathub/shared-modules.git`.
3. Open a pull request against `new-pr`; the Flathub reviewers will respond there.

After that the app lives in its own repository `github.com/flathub/io.github.crazmoe.StarCLX`; for new
versions, update the tag, commit and the newly generated `*-sources.json` there via pull request.

### Defaults for all users (rollout)

Admins set system-wide settings in `/etc/xdg/starclxrc`, in the format of
KDE config files. A commented template with all settings is in
[`packaging/vorlagen/etc/xdg/starclxrc`](packaging/vorlagen/etc/xdg/starclxrc) (comments in German),
installed under `/usr/share/doc/starclx/vorlagen`.

```ini
[General]
# initial value: applies until the user changes it
server=https://pbx.example.com
default_country_code=49
# locked (KDE kiosk): always applies, greyed out in StarCLX
autostart[$i]=true
verbose_log[$i]=false
```

- `key=value` is an initial value for new users, `key[$i]=value` locks the
  setting. `[General][$i]` locks all entries of the group.
- With `server` locked, no other system can be signed in to.
- All directories from `$XDG_CONFIG_DIRS` apply (`/etc/xdg` if unset); the first one takes precedence.
- On KDE you can also use `kwriteconfig6 --file /etc/xdg/starclxrc --group General --key server https://pbx.example.com`.
- The Flatpak only sees the host's `/etc` with
  `sudo flatpak override --system --filesystem=host-etc:ro ch.crazmoe.StarCLX`.

More templates in `packaging/vorlagen/etc/xdg`: `autostart/starclx.desktop` starts StarCLX
for everyone at login, `mimeapps.list` opens `tel:` links with StarCLX for everyone (GNOME and KDE).

### Door cameras with RTSP

StarCLX decodes door cameras with an RTSP stream (URL starts with `rtsp://`, H.264 video) with
`ffmpeg`. The packages do not pull it in automatically; install it once:

```sh
sudo apt install ffmpeg      # Debian, Ubuntu
sudo dnf install ffmpeg      # Fedora, from RPM Fusion (ffmpeg-free cannot do H.264)
```

Without ffmpeg the camera shows "RTSP cameras need ffmpeg". Cameras with Motion JPEG
or still images do not need it. RTSP does not work in the Flathub variant because it may not start
programs on the host. A camera can be tested without the app:
`cargo run -p sf-doorcam --example probe -- <URL>`.

### Desk phone control

Like the STARFACE app, StarCLX dials through the **primary phone**, which can be selected in the
profile menu. When dialing, the system first calls this phone:

- **Softphone as primary phone:** the softphone answers the system's callback
  by itself.
- **Desk phone (or another phone of your own) as primary phone:** the system rings it
  first; whoever picks up is connected to the dialed number. This applies to everything that
  dials: search, call list, contacts, chat, voicemail callback, function keys, keyboard shortcuts and
  `tel:` links. Pickup, park, voicemail via phone and recording announcements also go
  through this phone.

The Call Manager shows all of your own calls on the system, including those on the desk phone and even when the
softphone is turned off or not registered. Hang up, reject, hold, consultation,
transfer, conference, redirect, voicemail, recording, DTMF and Call2Go work there as well.

If the softphone is the primary phone, StarCLX hands this role back to the phone that was primary
before when you quit, sign out or turn off the softphone (if that phone no longer exists,
to the first other phone of your own). The system does not switch this by itself. If StarCLX crashes
or the computer is powered off hard, the softphone stays primary.

Limitations: answering only works on the softphone, because the system offers no remote pickup;
on the desk phone you pick up the handset. Mute is likewise only available on the softphone. Without the softphone,
StarCLX plays no ringtone; the desk phone rings by itself.

### Requirements on the phone system

- STARFACE 10 with reachable OneHub port 9092 and SIP/TLS
- User's phone type: "UCC Client for Linux" (created on first sign-in)
- For Call2Go, a configured iFMC number

## Releasing a new version

Bump the version in `Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json` and `apps/desktop/package.json`,
add a section to `CHANGELOG.md` and merge. The first build on `main` with the new
version creates the GitHub release `v<version>` with packages and the CHANGELOG section. Pushing a
tag `v<version>` by hand does the same.

## Structure

| Path | Contents |
|---|---|
| `proto/` | OneHub protos reconstructed from the STARFACE App for Windows (version in `proto/VERSION`) |
| `crates/sf-proto` | gRPC client stubs generated from them |
| `crates/sf-auth` | OAuth2 login (authorization code + PKCE, refresh), keyring |
| `crates/sf-onehub` | Connection to the OneHub API (port 9092) with bearer token |
| `crates/sf-tls` | TLS configuration including confirmed certificates |
| `crates/sf-core` | Session, address book, call list, function keys, reachability, voicemail |
| `crates/sf-sip` | Softphone based on libbaresip |
| `crates/sf-audio` | Audio devices, ringtones, test tone (PulseAudio/PipeWire) |
| `crates/sf-chat` | XMPP chat |
| `crates/sf-busylight` | Kuando Busylight via hidraw |
| `crates/sf-headset` | Headset buttons (USB HID telephony) via hidraw |
| `crates/sf-doorcam` | Door cameras: RTSP (via ffmpeg), Motion JPEG, still image |
| `apps/desktop` | Desktop app: Tauri 2, UI in Svelte 5 / TypeScript |
| `apps/sfctl` | Command line for scripts, see its [README](apps/sfctl/README.md) (in German) |

Further documentation under [`docs/`](docs) is in German.

## Development

Requirements: Rust (stable), Node 22, and on Debian/Ubuntu
`libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libdbus-1-dev`
(full list in the "Systempakete" step of `.github/workflows/ci.yml`).

```sh
cargo test --workspace
cd apps/desktop && npm install && npm run tauri dev
```

UI strings go through `t()` (the German text is the key); translations live in
`apps/desktop/src/lib/i18n/`. `npm run i18n:check` reports missing entries.

`sfctl` (command line for scripts: chat, status, calls with an announcement) is described in
[apps/sfctl/README.md](apps/sfctl/README.md) (in German).

## License

MIT, see [LICENSE](LICENSE).
