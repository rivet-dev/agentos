/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_setflags
#undef posix_spawnattr_setflags
#endif
int (*foo)(posix_spawnattr_t *, short) = posix_spawnattr_setflags;
int main(void) { return 0; }
