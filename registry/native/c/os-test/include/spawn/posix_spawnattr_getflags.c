/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_getflags
#undef posix_spawnattr_getflags
#endif
int (*foo)(const posix_spawnattr_t *restrict, short *restrict) = posix_spawnattr_getflags;
int main(void) { return 0; }
