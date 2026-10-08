# g13pad notices

Original g13pad code and modifications, including the editor and integration tools,
are licensed under **GNU GPL version 3 or later** (`GPL-3.0-or-later`). See LICENSE.
Copyright 2026 g13pad contributors. There is no warranty.

The combined driver contains upstream public-domain source and MIT-licensed helpers.
Their original notices remain in `driver/helper.cpp`, `driver/helper.hpp`, and
`driver/UPSTREAM-README.md`. Do not remove them when redistributing the combined project.
See [provenance](docs/provenance.md) for the exact source baseline and modifications.

`LICENSES/rust/` contains the locked Linux crates' notices and license expressions.
Crates with alternative licenses retain their alternatives; g13pad's original code is
GPLv3-or-later regardless of those alternatives. Included egui font data retain their own font
licenses, including the Ubuntu Font License and SIL Open Font License. System-selected
fonts are not included in this repository. NOTICE does not relicense those font files.

libusb and log4cpp are dynamically linked system dependencies under LGPL terms;
libevdev is a system dependency under MIT. xboxdrv is a separate optional executable
under GPLv3-or-later and is not bundled. Distribution of binary packages must retain the
applicable dependency notices and provide the corresponding source required by their
licenses as well as this project's GPLv3 source. See the Free Software Foundation's
[compatibility explanation](https://www.gnu.org/licenses/license-compatibility.en.html).

The neutral LCD logo is original rendered text, not the upstream branded bitmap.
Upstream product photographs, game artwork and sample bind files are omitted. Logitech
and Xbox names identify hardware/protocol compatibility; this project is independent.
