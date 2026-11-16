//! `cargo xtask measure --help`, apart from the code it describes.

/// `cargo xtask measure --help`: what is measured, and how.
pub(crate) const LONG_ABOUT: &str = "\
Measure what a rinch program (a shell component) costs, in isolated sessions.

Each run starts the program as the only client of a fresh nested compositor
(the session `cargo xtask session` describes) with RINCH_PERF=1, and reports:
  - CPU time (user+system) up to its first presented frame, in all and per
    thread, threads that ended before it included;
  - memory once settled: Rss, Pss, anonymous, file, shmem and swap; the
    resident set by what is mapped (the Vulkan driver, LLVM, fonts, GTK, the
    program's binary, the heap); and GPU memory from the DRM fdinfo, which is
    not in Rss;
  - over an idle window: CPU time per minute, wake-ups and preemptions per
    second, frames, and CPU per frame; and, in each run's notes, the
    threads that woke most in it, by name;
  - the renderer that presented: rinch's software renderer, or the GPU
    renderer on the adapter wgpu chose, marked when that is a software
    Vulkan driver.

--instructions runs the program under Valgrind's Callgrind and reports
instructions to the first frame, in the idle window, and per frame: the
deterministic figure, for a loaded machine. The idle window begins --settle
seconds after the first frame there too, and each run notes the
instructions of the settle, kept out of the window. A row gives the
instructions to the end of the settle, start-up and settle together: the
figure to compare across pins when a start-up thread may finish on either
side of start-up's end, since its work then moves between the two and only
their sum stays put. Without it, CPU time is the
figure to compare; wall time is printed apart, beside the load average, and
is not comparable.

Each session starts with an empty shader cache (--shader-cache cold);
--shader-cache warm fills one with an unmeasured start first and keeps it,
outside the sessions, for every run.

Under sway and labwc the compositor renders with pixman and offers no dmabuf,
so wgpu would offer only a software Vulkan driver (lavapipe), which rinch's
default renderer declines for its software renderer. --renderer gpu under
sway or labwc therefore gets a session whose compositor renders on the GPU
(wlroots' GLES2 renderer on the GPU's render node), and fails where the
machine has none. KWin composites with the GPU either way.

--summon ARGS --dismiss ARGS measures showing instead of starting: the
program starts hidden, settles, and is shown --shows times by running it
again with the summon arguments, hidden again with the dismiss arguments
after each show. Each show reports CPU time (or with --instructions the
instructions) from the summon command to the first frame after it, the
first show cold, each later one after a hide. The summon command's own
process is not counted. Settled means its threads ran almost nothing and
rinch prepared no frame for its hidden window over a second; the prepared
frames before each show are reported, and with --instructions each hide
too, from the dismiss command to settled, with its prepared frames. Memory
is read once, hidden and settled, before the first show (without
--instructions); the idle window is not measured.

A quiet second does not say the program has started: one starved of CPU
before its first turn passes it having done nothing, and its first show then
pays for its start. --prepares, for a program that prepares its hidden
window (rinch's [PERF] prepared lines), makes the first settle wait for its
first preparation, and fails the run, recording nothing, if none comes within
--first-frame-timeout. Without it, a run whose show 1 came before any
prepared frame says so in its notes.

--type TEXT, with --summon, types TEXT into the program after each show,
one key at a time, and measures each key: from the key to settled, CPU time
(or with --instructions the instructions, each key read from its own dump),
and in all. The keys come from the input client (cargo xtask input) that
measure runs in the session, started before the program and granted the
input KWin restricts. The program must say
two things in its output, and --type requires the words of both, with no
default: --keyboard-line TEXT, a line it writes once a key typed reaches
where it types (the first key waits for it, after the show's first frame),
and --query-line PATTERN, a line it writes at the hide with {} where it
gives how many characters it was typed (checked after the dismiss). A run
fails, recording nothing, if the keyboard line never comes, if a key leads
to no frame, or if the query line is missing or gives another length than
TEXT's: a key lost would otherwise make the figure that of a shorter query.

--place-pointer POINT --move-pointer POINT... moves the pointer, through
the same input client: first to the --place-pointer point, unmeasured, the
program then left to settle; then to each --move-pointer point in order,
each a step measured from the move to settled (CPU time, or with
--instructions the instructions, each step from its own dump), and in all,
with the frames. A point is X,Y on the session's first output or N:X,Y on
its Nth, in that output's logical pixels; an N past --outputs is refused
before any session starts. The steps come after the idle window, with any
--window window kept open through them, so the idle figures are those of a
run without them; with --summon, after each show (and its typed query). A
run fails, recording nothing, if a step leads to no frame within
--first-frame-timeout, or if the client refuses a step or ends.

--step-command CMD (repeatable) runs CMD as a client of the session, through
sh -c with the session's environment, as a measured step: from just before
it to settled, CPU time or with --instructions the instructions, each from
its own dump, in the order given, after the idle window and the pointer's
steps (sending the program a notification, or copying a second
configuration over the one --config seeded). Its own process is not
counted. Its frames are every window's, and the windows it opened and
closed are in the run's notes, since its effect may be on any window or may
open or close one. It must lead to a frame on some window, or to a window
opened or closed (a command that only closes a popup does), within
--first-frame-timeout; a command that ends with a status other than 0, or
does not end within that timeout, fails the run with its output. Its rows
are \"command step K of N\", the commands listed in the header. Not with
--summon.

Each step, a key of --type's or a pointer move, reports its frames and how
many of them ran reactive work (a signal notified, as rinch's frame lines
say). A reactive frame is no anomaly in itself: typing changes a query. With
two runs or more, a run whose pair for a step (frames presented, of them
reactive) matches no other run's is noted, with both pairs (\"2 presented, 1
reactive, against 1 and 0 in the other runs\"): the runs differ in that
step, and the dearer figure may include work that is not the step's, such as
a clock's minute. With one run nothing is
noted, and the rows are the reader's to compare. The frames are read once
the step is measured (under --instructions, once its dump is written), so a
frame presented before the dump is counted; a frame's line comes when it is
presented, after its work, so one whose work came just before the step
began is counted in it too.

--window APP_ID opens one window with that app id in the session at the
idle window's start (once it has had its first configure and drawn), and
keeps it open to its end: the idle figures then include what it costs the
program, and the report also prints the idle window's wake-ups and frames
as totals. A run with it minus one without is the cost of a window
appearing plus what a window held open costs over the window. A run fails
if the window cannot be opened or is gone by the end.

Under KWin a program that is one of the shell's components (by its file
name, or as --component PACKAGE names it) is granted what deskrinch-shell
grants that component, as the desktop grants it; anything else is granted
nothing. Each run's notes say what was granted, or why nothing.

--env KEY=VALUE (repeatable) sets a variable in the measured program's own
environment, such as MALLOC_ARENA_MAX=1, so that the program KWin sees and
grants is the program; the header and each run's notes say what was set.

Each run keeps the program's whole output in a file its notes name. A run
whose program logged an error fails, printing the lines: a program that
says it is broken is not measured. --allow-errors measures it anyway, with
the lines in the notes.

--state DIR starts each run's state directory (XDG_STATE_HOME) with a copy
of DIR's contents, such as the launcher's record of starts
(deskrinch/starts.toml), and --config DIR its configuration directory
(XDG_CONFIG_HOME), such as the wallpaper's [wallpaper] with a picture
(deskrinch/config.toml). The copy is made once the compositor has started
and before the program does, and nothing is written back to DIR. The
report's header says what was seeded, from where, and the files it held. A
DIR that is not a readable directory fails before any session.

--windows N measures a program that opens N windows (the wallpaper opens
one per output: --outputs 2 --windows 2), from rinch's per-window lines;
without it the main window's frames are read, as always. Start-up ends when
each of the N windows has presented a first frame, and \"CPU to first frame\"
runs to the last of them. Each window, named by its title and output, gets
the process's CPU (and, apart, the wall time) when its first frame came:
cumulative, work for every window included, not what that window cost, as
a line under those rows says. The idle window's frames are given per
window and in all. A run fails, with no report, when fewer than N windows
present within --first-frame-timeout (naming each window seen), when more
than N are open, when two have one name, and when a window closes or opens
after start-up (an output turned off and on again): measuring a program
whose windows change is outside what measure does, since its start-up and
idle figures would mix windows. N is given, never taken from --outputs:
how many windows a program opens per output is its own. Not with --summon.

--start-ends WHEN moves where start-up ends, and with it what the figures
run to and when the settle and the idle window begin: first-frame (the
default, as above); frame:N, the main window's or each expected window's
Nth presented frame, sound only for a program whose frames come in a fixed
order (the wallpaper's picture is in its first frame or its second by the
decode's timing, so frame:2 is not its picture); counter:NAME, the main
window's or each expected window's first presented frame whose rinch frame
line carries the counter NAME above zero, whichever frame it was (the
wallpaper's picture shown is --windows 2 --start-ends counter:image_draws,
once the pin has rinch's image_draws counter, rinch issue 2769);
line:TEXT, the first line of the program's output containing TEXT;
ready, the readiness deskrinch-shell declares for the component (by its file
name, or --component), owning its bus name on the session's bus, refused
for a component that is ready once started; bus-name:NAME, owning NAME, for
a program that is not a component. Any end but the default names its rows
(\"CPU to start-up's end\") and the header says which end it was. The idle
figures of a run with a later end and of a default run of the same program
are taken from different moments, so compare them with care: the
wallpaper's default run on the software renderer often counts the picture's
frame in its settle. A
program that opened no window in any run (a daemon with nothing to show,
ended by ready) is said so in the header, so that its frame rows of 0 are
not read as a failure to draw. An end that does not come within
--first-frame-timeout fails the run, naming it. Not with --summon.

Needs valgrind and callgrind_control for --instructions.";
