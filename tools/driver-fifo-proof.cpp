// SPDX-License-Identifier: GPL-3.0-or-later
// Filesystem-only check: never discovers USB, creates uinput or accesses live FIFOs.
#include <cassert>
#include <cerrno>
#include <fcntl.h>
#include <fstream>
#include <iostream>
#include <string>
#include <sys/stat.h>
#include <unistd.h>

namespace G13 { int G13CreateFifo(const char *); }

static int inject_failure = 0, checked_fd = -1;
extern "C" int __real_fstat(int, struct stat *);
extern "C" int __wrap_fstat(int fd, struct stat *status) {
  const int result = __real_fstat(fd, status);
  if (result == 0 && inject_failure) {
    checked_fd = fd;
    if (inject_failure == 2) {
      errno = EIO;
      return -1;
    }
    ++status->st_ino;
    errno = EBUSY; // An unrelated prior error must not describe an identity mismatch.
  }
  return result;
}

int main() {
  char directory[] = "/tmp/g13-fifo-proof-XXXXXX";
  assert(mkdtemp(directory));
  const std::string root(directory), pipe = root + "/pipe", target = root + "/target";
  int fd = G13::G13CreateFifo(pipe.c_str());
  assert(fd >= 0);
  struct stat status {};
  assert(fstat(fd, &status) == 0 && S_ISFIFO(status.st_mode));
  assert((status.st_mode & 0777) == 0660);
  assert(fcntl(fd, F_GETFD) & FD_CLOEXEC);
  close(fd);
  assert(chmod(pipe.c_str(), 0777) == 0);
  fd = G13::G13CreateFifo(pipe.c_str());
  assert(fd >= 0 && fstat(fd, &status) == 0 && (status.st_mode & 0777) == 0660);
  close(fd);
  for (const int failure : {1, 2}) {
    inject_failure = failure;
    assert(G13::G13CreateFifo(pipe.c_str()) == -1);
    assert(errno == (failure == 1 ? EACCES : EIO));
    inject_failure = 0;
    assert(fcntl(checked_fd, F_GETFD) == -1 && errno == EBADF);
    assert(lstat(pipe.c_str(), &status) == 0 && S_ISFIFO(status.st_mode));
  }
  unlink(pipe.c_str());
  { std::ofstream file(target); file << "preserved"; }
  assert(chmod(target.c_str(), 0600) == 0);
  assert(G13::G13CreateFifo(target.c_str()) == -1);
  assert(symlink(target.c_str(), pipe.c_str()) == 0);
  assert(G13::G13CreateFifo(pipe.c_str()) == -1);
  assert(stat(target.c_str(), &status) == 0 && (status.st_mode & 0777) == 0600);
  std::ifstream file(target); std::string text; file >> text; assert(text == "preserved");
  unlink(pipe.c_str()); unlink(target.c_str());
  assert(rmdir(directory) == 0);
  std::cout << "FIFO permissions/reuse, path refusal, identity/syscall errors and failed-open cleanup passed\n";
}
