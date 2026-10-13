# Doom engines

Moved verbatim from the project AGENTS.md on 2026-10-13 (Claude, Fable 5.1); the
nudges stayed there, the runbook lives here. Dates in the text are the original ones.

**Verified against:** Zandronum 3.2 and GZDoom, 2026-10-08.
**Newer versions:** attempted, never refused (asked 2026-10-13): the reader is read-only, so it
tries the last known layout under its validation; if that fails the display is honestly empty
and the user turns the LCD to another mode. Update the line above when a newer version is
checked; do not add a version gate.

## Doom-family engines: the console is the channel

ZDoom-family engines (Zandronum, GZDoom) have no integration feed, but ACS can print
to the console and the console can go to a file (`+logfile PATH`). `contrib/doom-health`
is a tiny ACS library loaded with every game through `LOADACS` that prints a tagged
line, dark grey, only on a change (the console is the player's too; the follower keeps
the feed alive between lines); health mode `log` makes the watcher follow the newest
file with that name. Lessons: a PK3 is a zip, and `bsdtar -a` writes a *tar* for an
unknown extension (the engine then lists the file with no lumps and nothing runs);
an ACS library must start with `#library "NAME"`, or its string constants resolve
against another mod's table (with Brutal Doom loaded the tag came out as its weapon
strings with my numbers between); Zandronum appends a timestamp to the log name unless
`sv_logfilenametimestamp` is off; Zandronum 3.2 returns 0 for `APROP_SpawnHealth`; the console is not on stdout
there, so look at the logfile, and test on a private X display with a copied config
(`-config`), because the engine rewrites its ini on exit.
