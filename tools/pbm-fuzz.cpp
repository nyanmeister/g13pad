// SPDX-License-Identifier: GPL-3.0-or-later
// Exercise the actual converter entrypoint using memory streams, without processes.
#define main g13pad_pbm_main
#include "../driver/pbm2lpbm.cpp"
#undef main
#include <cassert>
#include <cstdlib>
#include <cstdint>
#include <sstream>

extern "C" int LLVMFuzzerInitialize(int *argc, char ***argv) {
  if (*argc == 2 && std::strcmp((*argv)[1], "--version") == 0) {
    std::cout << "g13pad pbm-fuzz " << GIT_VERSION << '\n';
    std::exit(0);
  }
  return 0;
}

extern "C" int LLVMFuzzerTestOneInput(const uint8_t *data, size_t size) {
  if (size > 16384) return 0;
  std::istringstream input(std::string(reinterpret_cast<const char *>(data), size));
  std::ostringstream output, errors;
  auto *old_input = std::cin.rdbuf(input.rdbuf());
  auto *old_output = std::cout.rdbuf(output.rdbuf());
  auto *old_errors = std::cerr.rdbuf(errors.rdbuf());
  std::cin.clear();
  char name[] = "pbm2lpbm";
  char *argv[] = {name, nullptr};
  const int result = g13pad_pbm_main(1, argv);
  std::cin.rdbuf(old_input);
  std::cout.rdbuf(old_output);
  std::cerr.rdbuf(old_errors);
  std::cin.clear();
  const auto frame = output.str();
  if (result != 0) {
    assert(frame.empty());
    return 0;
  }
  assert(size >= 860 && frame.size() == 960);
  // Decode column pages independently back to row-major PBM bits.
  const auto *pixels = data + size - 860;
  for (unsigned y = 0; y < 48; ++y) {
    for (unsigned x = 0; x < 160; ++x) {
      const auto actual = (static_cast<unsigned char>(frame[x + y / 8 * 160]) >> (y % 8)) & 1;
      const auto expected = y < 43 ? (pixels[y * 20 + x / 8] >> (7 - x % 8)) & 1 : 0;
      assert(actual == expected);
    }
  }
  return 0;
}
