// SPDX-License-Identifier: GPL-3.0-or-later
// Stand-in for Steam's unversioned dlsym hook. Its constructor needs that hook
// to receive a lookup before startup completes. Bypassing it with dlvsym breaks
// this contract, just as it leaves the overlay's close hook uninitialized.
#include <dlfcn.h>
#include <unistd.h>
namespace {
bool dispatched;
using Lookup = void *(*)(void *, const char *);
}
extern "C" void *dlsym(void *handle, const char *name) noexcept {
    dispatched = true;
#ifdef __i386__
    Lookup lookup = (Lookup)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.0");
#else
    Lookup lookup = (Lookup)dlvsym(RTLD_NEXT, "dlsym", "GLIBC_2.2.5");
#endif
    return lookup ? lookup(handle, name) : nullptr;
}
__attribute__((constructor)) static void bootstrap() {
    if (!dlsym(RTLD_NEXT, "malloc") || !dispatched) _exit(91);
}
