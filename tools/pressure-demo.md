# System pressure preview

System pressure is now a watcher-owned profile display. Choose **System pressure**
beside **Health mode** in the editor; the ticks are mutually exclusive. See
[System pressure](../docs/system-pressure.md) for usage and interpretation.

This helper previews the same renderer without opening the daemon or taking LCD
ownership. The former `--live` path is retired because it blocked editor opening.

```sh
rustc --edition=2021 -D warnings -O tools/pressure-demo.rs -o /tmp/g13-pressure-demo
/tmp/g13-pressure-demo --version
/tmp/g13-pressure-demo --seconds 15 --preview /tmp/pressure.pgm
/tmp/g13-pressure-demo --synthetic --preview /tmp/pressure-example.pgm
```

PGM output has a dark background at native 160×43 resolution. Synthetic values and
history are explicitly marked `DEMO`. The helper uses no GUI, service or hardware
and generates no artificial load. For collector/renderer tests:

```sh
rustc --edition=2021 -D warnings --test tools/pressure-demo.rs -o /tmp/g13-pressure-demo-tests
/tmp/g13-pressure-demo-tests
```
