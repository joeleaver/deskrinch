# deskrinch

A Wayland desktop built on [rinch](https://github.com/joeleaver/rinch): a shell
first (panel, launcher, notifications on `wlr-layer-shell`, running under KWin),
then a smithay-based compositor.

Early days: the libraries the shell rests on, a panel (`deskrinch-panel`),
and the launcher's process (`deskrinch-launcher`: `deskrinch-launcher toggle`
from a key binding shows and hides it; its view is still to come).

The configuration file, `~/.config/deskrinch/config.toml`, is documented in
the `deskrinch-settings` crate (`cargo doc -p deskrinch-settings --open`).

## Building and checking

`rust-toolchain.toml` pins the compiler; rustup installs it on first use. CI
runs exactly these, and a pull request merges only when all pass:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
cargo deny --locked check      # cargo install cargo-deny --locked
cargo xtask file-length
```

CI runs them on Ubuntu 26.04, each job within a time limit. A job that
fails in its "Install system packages" step with "Package servers
unreachable" never reached the code: the runner could not reach Ubuntu's
package servers, and re-running the job is the remedy.

Building rinch, which the panel uses, needs the GTK 3 and libxdo development
files for its native menus (on Debian and Ubuntu, `libgtk-3-dev` and
`libxdo-dev`).

The desktop-entries crate's tests start a private `dbus-daemon` (from the
`dbus` package) for the systemd-scope and D-Bus activation paths; it talks
to nothing in your session.

The notification service's tests also start private D-Bus daemons, and run
libnotify's `notify-send` 0.8.8 or newer against them (on Debian and Ubuntu,
the `libnotify-bin` package). They never touch the session bus.

The appearance crate's tests run a stand-in settings portal on a private
D-Bus daemon too, never the session's.

The compositor tests (the window-management crate's, `deskrinch-test-screen`'s,
`deskrinch-test-input`'s, the launcher's and the panel's) drive a real
compositor and are ignored by `cargo test`. This runs them in nested,
headless KWin, sway and labwc sessions that touch nothing in your own (the
isolated sessions `cargo xtask session` gives; it needs `dbus-daemon`,
`kwin_wayland`, `sway`, `labwc`, and KDE's `kscreen-doctor` for KWin at a
scale; name compositors to run only those). The screen tests run twice per
compositor, at scale 1 and at scale 2:

```sh
cargo xtask compositor-tests [kwin] [sway] [labwc]
cargo xtask compositor-tests --renderer software sway
```

The suites that start a real component (the panel's) run once per renderer,
and each of their tests checks which one presented. `--renderer software`
runs rinch's software renderer only, as CI does; `--renderer gpu` the GPU
renderer only, and fails on a machine without a GPU; `all` (the default)
both, the GPU only where the machine has one. Headless sway and labwc
composite with pixman, which offers no dmabuf, so the GPU renderer has
nothing to present on there: their GPU pass runs in one more session per
pass whose compositor renders on the GPU (`--compositor-renderer gpu`, below),
and only the rendering suites run in it. Whether the machine has a GPU
is a guess from `/sys/class/drm/renderD*`, a render node with a driver other
than vgem; the run prints which renderers it chose and why. A test binary
reads the run's renderer from `DESKRINCH_TEST_RENDERER`
(`deskrinch_test_component::RENDERER_VARIABLE`).

The panel's tests start the real `deskrinch-panel` in the session and check
by screen pixels that it sits at its configured edge and height on exactly
one output, that a maximised test window stops at the zone it reserves, and
that an edit of its settings file moves and resizes it (a rejected value
leaving it where it was). `deskrinch-test-component` starts a component for
such a test, with the run's renderer, and follows its log: its frames, the
renderer that presented, and lines it waits for.

Under KWin, on Debian and Ubuntu, that means `kwin-wayland` and `kwin-common`
(KWin's plugins, its screenshot interface among them, which `kwin-wayland`
only recommends), `libkscreen-bin` and `libqt6wlshellintegration6` (for
`kscreen-doctor`), and a KWin that composites with OpenGL, which the session
checks. On a machine without a GPU that takes vgem's DRM device, open to your
user, and Mesa's software driver (`libgl1-mesa-dri`); the workflow's KWin job
(`.github/workflows/ci.yml`, "Give KWin a DRM device to render on") has the
exact step.

### Checking what is on screen

A compositor test that checks what a component draws captures an output with
`deskrinch-test-screen` (a dev-dependency) and compares pixels; its windows of
known content come from `deskrinch-test-client`:

```rust
let mut screen = deskrinch_test_screen::Screen::connect()?;
let output = &screen.outputs()[0];
let image = screen.capture_until(output, PATIENCE, |image| {
    image.pixel(0, 0) == Some(Rgba::opaque(30, 30, 30))
})?;
```

An `Image` is the output's physical pixels, upright, as `Rgba` rows from the
top. A capture the compositor does not finish within a minute fails with
`CaptureError::TimedOut`, naming the output, instead of hanging the test. The crate captures through `ext-image-copy-capture` (sway, labwc),
`wlr-screencopy`, or KWin's `org.kde.KWin.ScreenShot2`, whichever the
compositor offers. Under KWin the test binary must be granted the screenshot
interface: `cargo xtask compositor-tests` installs a desktop file for each test
binary, built with `deskrinch-kwin-access` from the interface constants of the
crates it uses (`deskrinch_test_screen::KWIN_SCREENSHOT_INTERFACE`, ...). The
screen tests run each compositor's outputs at scale 1, at scale 2, turned a
quarter, and mirrored and turned.

### Typing into a window

A compositor test that types presses keys with `deskrinch-test-input`, as a
keyboard would, so the compositor hands them to the window with the keyboard
focus. Make the `Input` before the windows that are to hear it: on a headless
wlroots seat its virtual keyboard is the seat's only keyboard.

```rust
let mut input = deskrinch_test_input::Input::connect()?;
let window = client.open("typed into", "org.deskrinch.Example")?;
window.wait_keyboard()?;
input.type_text("ok")?;
input.press(Key::ENTER)?;
window.wait_key(Key::ENTER.code())?;
```

It types through wlroots' `zwp_virtual_keyboard_v1` (sway, labwc) or KWin's
restricted `org_kde_kwin_fake_input`, which `cargo xtask compositor-tests`
grants the test binary like the screenshot interface
(`deskrinch_test_input::KWIN_FAKE_INPUT_INTERFACE`). A test window
(`deskrinch-test-client`) records the keys pressed while it has the focus, by
evdev code.

### Pointing and clicking

The same `Input` moves a pointer and presses its buttons. A point is named on
an output, in that output's logical pixels from its top-left corner (the
coordinates a fullscreen window on it has, at any scale or transform), with
the output names `deskrinch-test-client` and `deskrinch-test-screen` use:

```rust
input.move_pointer("HEADLESS-2", 120.0, 80.0)?;
window.wait_pointer_at(120.0, 80.0)?;
input.click(Button::PRIMARY)?;
```

`press_button` and `release_button` hold and let go of a button apart, for a
press on one thing and a release on another. Under sway and labwc it goes
through wlroots' `zwlr_virtual_pointer_v1`, under KWin through the same fake
input as the keys. A test window records what it hears of the pointer
(entering, moving, buttons, leaving) in its own surface coordinates. The
pointer jumps straight to each point; it cannot scroll, touch or move
relatively.

One desktop-entries test launches into a real transient scope of your
systemd user instance, and is ignored by `cargo test`. It starts only its
own recorder script, and stops the scope again:

```sh
cargo xtask systemd-scope-tests
```

### Running something under a nested compositor

`cargo xtask session` is the one way to run anything under a nested
compositor (a shell component, a rinch example, a screenshot tool): never a
hand-made session. It starts a fresh headless KWin, sway or labwc, runs the
command as its client, and when the command exits ends the whole session:

```sh
cargo xtask session sway sh -c 'foot & sleep 2; grim shot.png'
cargo xtask session --outputs 2 kwin sh -c 'foot --maximized & sleep 3; spectacle -b -n -f -o shot.png'
```

What the command, and everything it starts, sees:

- `HOME`, `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, `XDG_CONFIG_HOME`,
  `XDG_CACHE_HOME` and `XDG_STATE_HOME`: private directories under a fresh
  `/tmp/deskrinch-session-*` (also `$DESKRINCH_SESSION_DIR`).
- `XDG_CONFIG_DIRS=/etc/xdg` and `XDG_DATA_DIRS=/usr/local/share/:/usr/share/`,
  the XDG specification's defaults, not yours.
- `CARGO_HOME` and `RUSTUP_HOME` at your real ones, so cargo and rustup work.
- `WAYLAND_DISPLAY` naming the nested compositor (and `SWAYSOCK` its IPC
  under sway). There is no X server: KWin runs without Xwayland.
- `DESKRINCH_TEST_COMPOSITOR` naming the compositor (`kwin`, `sway` or
  `labwc`), which the compositor tests' guard against the real session reads.
- `DBUS_SESSION_BUS_ADDRESS` naming a private `dbus-daemon` with service
  activation off, which the compositor uses too: no portal, secret service
  or notification daemon is ever started on demand.
- `XDG_CURRENT_DESKTOP` only when `--current-desktop NAME` sets it.
- With `--compositor-renderer gpu`, sway and labwc render with wlroots'
  GLES2 renderer on the GPU's render node (`WLR_RENDER_DRM_DEVICE`; a render
  node only, no KMS, seat or logind) instead of pixman, which a client
  presenting through Vulkan needs; the session fails (70) on a machine
  without a GPU, and unless the compositor's own log says it created its
  GLES2 renderer on a GL that is not Mesa's software rasteriser. KWin
  composites with OpenGL either way.
- Under sway, no borders or title bars on any window, so that a tiled or
  maximised window fills the area it is given.
- None of your `DISPLAY`, `XAUTHORITY`, `WAYLAND_DISPLAY`, `WAYLAND_SOCKET`,
  `SWAYSOCK`, `I3SOCK`, `DBUS_SESSION_BUS_ADDRESS`, `XDG_CURRENT_DESKTOP`, or
  the login session's `XDG_SESSION_ID`, `XDG_SESSION_CLASS`,
  `XDG_SESSION_DESKTOP`, `XDG_SESSION_TYPE`, `XDG_SESSION_PATH`, `XDG_SEAT`,
  `XDG_SEAT_PATH` and `XDG_VTNR`.
- Everything else as you have it (`PATH`, the locale, `TERM`, `CARGO_*` and
  `RUST*` settings, ...).

Caches follow `XDG_CACHE_HOME` and `HOME`, so a GPU client's shader cache
(Mesa's `mesa_shader_cache`, for one) is inside the session and starts empty
each time; set `MESA_SHADER_CACHE_DIR` in the command to keep one.

- Standard input closed, and the working directory you called it from.
- One process group with the compositor. When the command exits, or the task
  gets `SIGINT`, `SIGTERM` or `SIGHUP`, the group gets `SIGTERM`, then
  `SIGKILL`, and the bus is stopped. A process that escaped the group is then
  found by its environment, killed, and reported as a failure.

The command starts once the session is ready: the compositor's socket is
open, under KWin its D-Bus services (`org.kde.KWin`, and
`org.kde.KWin.ScreenShot2`) are on the bus, and every output reports the
scale asked for.

It exits with the command's status (128 + N for signal N), 130 when
interrupted, and 70 when the session could not be set up or did not end
cleanly. `--outputs N` sets the number of outputs (1280x720 each), `--keep`
keeps the session's directory (with the compositor's log);
`cargo xtask session --help` has the details.

`--scale S` (a positive decimal, such as `2` or `1.5`) puts every output at
that scale before the command starts, so it never sees scale 1: sway through
its configuration, labwc through `wlr-output-management`, and KWin with KDE's
`kscreen-doctor`, which a KWin session then needs (on Debian and Ubuntu,
`libkscreen-bin`, or `libkf5screen-bin` for Plasma 5). The session fails (70)
if an output then reports another scale: its whole scale on `wl_output`, and
its logical size on `xdg-output` against its mode (853x480 at 1.5).

`--transform T` turns every output the same way (`normal`, `90`, `180`, `270`,
`flipped`, `flipped-90`, `flipped-180`, `flipped-270`, as `wl_output` names
them, counter-clockwise): sway through its configuration, labwc through
`wlr-output-management`, KWin through `kscreen-doctor`. The session fails (70)
if an output reports another transform, or a logical size not turned with it.

From outside this repository, for example in a rinch clone under `.work/`,
name xtask's manifest; the command still runs in your directory:

```sh
cargo run -q --manifest-path ~/dev/deskrinch/xtask/Cargo.toml -- session sway sh -c '...'
```

Cargo takes its toolchain from the directory it runs in. A rinch checkout
pins the same toolchain as deskrinch, so this works there; elsewhere, add
`+<channel>` with the channel deskrinch's `rust-toolchain.toml` names.

Its own tests start only isolated sessions; they need a compositor, so they
are ignored by `cargo test`:

```sh
DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test session -- --ignored
```

### Measuring what a component costs

`cargo xtask measure` starts a rinch program (a shell component, a rinch
example) in fresh isolated sessions, one per run, and reports what it costs:

```sh
cargo build --release -p deskrinch-panel
cargo xtask measure target/release/deskrinch-panel
cargo xtask measure --shader-cache warm --renderer gpu target/release/deskrinch-panel
cargo xtask measure --instructions --runs 1 target/release/deskrinch-panel
```

For each run: the CPU time to the first presented frame, in all and per
thread (shader-compiling workers that have ended included); the memory once
settled, with the resident set grouped by what is mapped (the Vulkan driver,
LLVM, fonts, GTK, the binary, the heap) and the GPU memory the DRM driver
reports, which is not in the resident set; over an idle window (125 s, two
ticks of a minute clock), CPU time, wake-ups and preemptions per second,
frames and CPU per frame; and the renderer that presented, rinch's software
one or the GPU one on the adapter wgpu chose, marked when that is a software
Vulkan driver. Across runs it prints each number's median and range.

The figures to compare are CPU time and, with `--instructions` (the program
under Valgrind's Callgrind; needs `valgrind`), instruction counts, which do
not move with the machine's load. Wall time is printed apart, beside the load
average, as not comparable.

Each session's shader cache starts empty (`--shader-cache cold`, a first
start); `--shader-cache warm` fills one with an unmeasured start first, as
every later start in a real session finds it. KWin (`--compositor kwin`,
the default) composites with the GPU. Headless sway and labwc render with
pixman, which offers no dmabuf, so wgpu would offer only a software Vulkan
driver there, which rinch's default renderer declines for its software
renderer; `--renderer gpu` under them gets a session whose compositor
renders on the GPU instead (`--compositor-renderer gpu`). `--expect-renderer
software|gpu` fails (exit 1), naming what presented, unless every run
presented with that renderer; CI measures the panel once under sway with
`--expect-renderer software`. `cargo xtask measure --help` has the details.

A component that starts hidden and is summoned, such as the launcher, is
measured by its shows instead: `--summon` and `--dismiss` give the program's
own arguments that show and hide it, and each show's CPU time (or
instructions) is taken from the summon command to the first frame after it,
the first show cold and the later ones after a hide:

```sh
cargo build --release -p deskrinch-launcher
cargo xtask measure --renderer software --summon show --dismiss hide target/release/deskrinch-launcher
```

Its end-to-end tests measure stand-in scripts in isolated sessions, and are
ignored by `cargo test` like the session's (they do not cover
`--instructions`, which needs Valgrind):

```sh
DESKRINCH_SESSION_TEST_COMPOSITOR=sway cargo test -p xtask --test measure -- --ignored
```

Repository tasks are `cargo xtask` subcommands; `cargo xtask --help` lists
them. Local Cargo settings, such as a `[patch]` for unmerged rinch work, go in
the gitignored `.cargo/config.local.toml`.

Licensed under either of MIT or Apache-2.0, at your option.
