# System pressure

Click the screen in the editor and choose **Computer load** under **Shows**, then save the
profile. The watcher shows it while that profile is active and the editor is closed. A
profile shows one thing at a time; choosing **Picture** again returns to the profile's
saved picture or animation.
The picture is preserved when a live display is selected.
System pressure collects machine-wide data rather than data from the focused
program, so it stays separate from the program integration selector.

You can open the editor from the panel applet while System pressure is running.
It yields the LCD to the editor, then resumes after the editor closes. It does not
hold the editor's ownership lock, change the backlight or generate artificial load.
Temporary LCD error messages retain priority.

The command-line equivalent only saves the profile; normal `use` applies it:

```sh
g13map profile pressure default on
g13map use default
# To return to the saved picture:
g13map profile pressure default off
```

## Reading the screen

The rows show CPU, memory and I/O pressure. The number is the kernel's `some`
`avg10` percentage: time when at least one task was stalled for that resource,
averaged over approximately ten seconds. CPU pressure is not CPU utilization;
memory pressure is not RAM usage; I/O pressure is not transfer speed. High usage
can have zero pressure when work does not have to wait.

Each graph holds the most recent 90 seconds, newest at the right. Columns show
one-second stall percentages calculated from cumulative `some total` microseconds
and measured elapsed time. `STALL10S 90S/10%` means ten-second numbers, 90-second
history and a graph ceiling of 10%. The rows share a ceiling, which automatically
selects 10, 25, 50 or 100% to fit retained samples. A full column reaches that
labelled ceiling. Seven pixels limit precision; the lowest positive bar represents
a bin, not an exact percentage.

History starts empty. Missing counters show `--.-%` and gaps; valid zero samples
show a baseline. Delayed polling leaves missing seconds rather than compressing a
pause. Sampling continues while the editor owns the display, so history is current
when it closes. Kernel counters update asynchronously, so individual columns can
be noisier than the ten-second average.

Kernel reference: [Pressure Stall Information](https://docs.kernel.org/accounting/psi.html).

The hardware-free [preview tool](../tools/pressure-demo.md) uses this same collector
and renderer. Its former standalone hardware ownership path was retired because
it prevented the editor window opening during a demo.
