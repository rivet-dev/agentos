/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_setsigmask
#undef posix_spawnattr_setsigmask
#endif
int (*foo)(posix_spawnattr_t *restrict, const sigset_t *restrict) = posix_spawnattr_setsigmask;
int main(void) { return 0; }
