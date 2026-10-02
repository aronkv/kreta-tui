# kreta

A fast terminal client for **e-KRÉTA**, the Hungarian school administration system, written in Rust with [ratatui](https://ratatui.rs).
The interface is in Hungarian, like the system it talks to.

> Unofficial project. It is not affiliated with or endorsed by eKRÉTA Informatikai Zrt.
> It uses the same private API as the official mobile app, which may change without notice.

## Features

- **Áttekintés (overview)**: overall average with a 30-day trend, today's lessons with the current one highlighted, latest grades, upcoming tests and homework
- **Jegyek (grades)**: per-subject weighted averages and an average-over-time chart
  - What-if simulator: add hypothetical grades and see the new average
  - How many 5s you need to reach the next grade, and how many 1s you can afford before dropping one
- **Órarend (timetable)**: weekly grid with substitutions, cancelled lessons, test and homework markers, and week navigation
- **Hiányzások (absences)**: justified / pending / unjustified breakdown, lateness totals, per-subject chart
- **Teendők (to-do)**: announced tests, homework, teacher notes
- **Statisztika (statistics)**: average timeline, grade distribution, subject ranking, fun facts

On startup it renders instantly from a local cache, then refreshes in the background.

## Install

Linux (x86_64 / aarch64):

```sh
curl -fsSL https://raw.githubusercontent.com/DarkAaronfox/kreta-tui/main/install.sh | sh
```

This installs the `kreta` binary to `~/.local/bin` (override with `KRETA_INSTALL_DIR`).

To build from source instead (Rust 1.88+):

```sh
cargo install --git https://github.com/DarkAaronfox/kreta-tui
```

Uninstall:

```sh
curl -fsSL https://raw.githubusercontent.com/DarkAaronfox/kreta-tui/main/install.sh | sh -s -- --uninstall
```

## Usage

```sh
kreta            # log in, then browse your data
kreta --demo     # made-up data, no login needed
kreta --logout   # forget the saved session and cache
```

### Logging in

There are two ways to log in:

- **Password form**: enter your institute code (the subdomain of your e-KRÉTA address, `https://<code>.e-kreta.hu`), your student ID (*oktatási azonosító*), and your password.
- **Browser login (`Ctrl+B`)**: use this if KRÉTA asks for a captcha, or if you'd rather not type your password into a terminal.
  - If you have a Chromium-based browser (Chrome, Chromium, Brave, Vivaldi, Edge…), `kreta` opens a separate login window with a throwaway profile, detects the redirect, and closes the window when you're done.
  - Otherwise it opens your default browser and watches the clipboard. After logging in, copy the address of the page you land on (`mobil.e-kreta.hu/…?code=…`), or paste it into the terminal.

The password is never stored. Only the OAuth tokens are saved, to `~/.local/share/kreta-tui/session.json` (mode 0600). Downloaded data is cached in `~/.cache/kreta-tui/data.json`.

Log out from inside the app with `L`, or run `kreta --logout`.

### Keys

| Key | Action |
| --- | --- |
| `Tab` / `Shift+Tab`, `1`–`6` | switch tabs |
| `r` | refresh |
| `?` | help |
| `L` | log out |
| `q`, `Ctrl+C` | quit |
| **Grades** `↑↓` / `jk` | select subject |
| **Grades** `→` / `Enter` | focus grade list |
| **Grades** `s` | simulator: `1`–`5` add grade, `w` cycle weight, `⌫` undo, `c` clear |
| **Timetable** `←→↑↓` / `hjkl` | select lesson |
| **Timetable** `n` / `p` | next / previous week |
| **Timetable** `t` | back to today |

### Environment

| Variable | Effect |
| --- | --- |
| `KRETA_BROWSER` | browser binary used for the automatic browser login |
| `KRETA_BROWSER_FLAGS` | extra flags for that browser (e.g. `--ozone-platform=wayland`) |
| `KRETA_INSTALL_DIR` | install location used by `install.sh` |

## How it works

Login uses the same OAuth2 authorization-code flow with PKCE as the official iOS app
(`idp.e-kreta.hu`, client `kreta-ellenorzo-student-mobile-ios`). Data comes from
`https://<institute>.e-kreta.hu/ellenorzo/V3/Sajat/…`.

Prior art and references: [reFilc / Firka](https://github.com/QwIT-Development/app-legacy),
[ekreta-docs-v3](https://github.com/bczsalba/ekreta-docs-v3).

## License

MIT
