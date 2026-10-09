// SPDX-License-Identifier: GPL-3.0-or-later
// The key state file's text from raw input reports: bit order, the firmware bits left
// out, the stick's raw bytes. No USB, uinput or files.
#include "g13_device.hpp"
#include <cassert>
#include <cstring>
#include <iostream>
#include <string>

int main() {
  unsigned char report[8] = {};
  report[1] = 128;
  report[2] = 127;
  const int blue[3] = {0, 0, 255}, amber[3] = {255, 96, 0};
  assert(G13::G13KeyStateText(report, blue) == "stick 128 127\nbacklight 0 0 255\nkeys\n");
  report[3] = 0x01 | 0x80;         // G1, G8
  report[4] = 0x20;                // G14
  report[5] = 0x20 | 0x40 | 0x80;  // G22, UNDEF1, LIGHT_STATE
  report[6] = 0x01 | 0x02 | 0x20;  // BD, L1, M1
  report[7] = 0x01 | 0x08 | 0x10 | 0x20 | 0x40 | 0x80;  // MR, TOP, UNDEF3, LIGHT, LIGHT2, MISC_TOGGLE
  report[1] = 0;
  report[2] = 255;
  const std::string text = G13::G13KeyStateText(report, amber);
  std::cout << text;
  assert(text == "stick 0 255\nbacklight 255 96 0\nkeys G1 G8 G14 G22 BD L1 M1 MR TOP LIGHT\n");
  std::cout << "keystate-proof: ok" << std::endl;
  return 0;
}
