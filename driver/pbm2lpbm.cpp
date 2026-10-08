// Original conversion logic replaced with bounded parsing/packing for g13pad.
// SPDX-License-Identifier: GPL-3.0-or-later
#include "GIT-VERSION.h"
#include <array>
#include <charconv>
#include <cctype>
#include <cstring>
#include <iostream>
#include <string>

static bool token(std::string &text) {
  text.clear();
  char c;
  while (std::cin.get(c)) {
    if (std::isspace(static_cast<unsigned char>(c))) continue;
    if (c == '#') {
      unsigned length = 0;
      while (std::cin.get(c) && c != '\n') {
        if (++length > 1024) return false;
      }
      continue;
    }
    text.push_back(c);
    while (std::cin.get(c)) {
      if (std::isspace(static_cast<unsigned char>(c))) {
        if (c == '\r' && std::cin.peek() == '\n') std::cin.get();
        return true;
      }
      if (text.size() >= 32) return false;
      text.push_back(c);
    }
    return false;
  }
  return false;
}

int main(int argc, char *argv[]) {
  if (argc == 2 && std::strcmp(argv[1], "--version") == 0) {
    std::cout << "pbm2lpbm " << GIT_VERSION << '\n';
    return 0;
  }
  if (argc != 1) {
    std::cerr << "usage: pbm2lpbm < 160x43.pbm > frame.lpbm\n";
    return 1;
  }
  std::string text;
  unsigned width = 0, height = 0;
  const auto dimension = [&text](unsigned &value) {
    if (!token(text)) return false;
    const auto parsed = std::from_chars(text.data(), text.data() + text.size(), value);
    return parsed.ec == std::errc{} && parsed.ptr == text.data() + text.size();
  };
  if (!token(text) || text != "P4" || !dimension(width) || !dimension(height)
      || width != 160 || height != 43) {
    std::cerr << "expected a raw PBM (P4) header for exactly 160x43 pixels\n";
    return 1;
  }
  std::array<unsigned char, 860> input{};
  if (!std::cin.read(reinterpret_cast<char *>(input.data()), input.size())
      || std::cin.peek() != std::char_traits<char>::eof()) {
    std::cerr << "expected exactly 860 PBM pixel bytes\n";
    return 1;
  }
  std::array<unsigned char, 960> frame{};
  for (unsigned y = 0; y < 43; ++y) {
    for (unsigned x = 0; x < 160; ++x) {
      const auto bit = (input[y * 20 + x / 8] >> (7 - x % 8)) & 1;
      frame[x + (y / 8) * 160] |= bit << (y % 8);
    }
  }
  std::cout.write(reinterpret_cast<const char *>(frame.data()), frame.size());
  return std::cout.good() ? 0 : 1;
}
