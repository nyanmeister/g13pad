// SPDX-License-Identifier: GPL-3.0-or-later
// The first library is RTLD_LOCAL and depends on the second. Caller-relative
// lookup must see the first's own symbol with DEFAULT and the second's with NEXT.
#ifdef G13_SECOND
extern "C" int goldsrc_scope() { return 42; }
#else
#include <dlfcn.h>
extern "C" int goldsrc_scope() { return 11; }
extern "C" int check_lookup() {
    auto next = (int (*)())dlsym(RTLD_NEXT, "goldsrc_scope");
    auto current = (int (*)())dlsym(RTLD_DEFAULT, "goldsrc_scope");
    return next && current && next() == 42 && current() == 11;
}
#endif
