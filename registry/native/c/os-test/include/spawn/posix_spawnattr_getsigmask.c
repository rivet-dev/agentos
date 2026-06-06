/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_getsigmask
#undef posix_spawnattr_getsigmask
#endif
int (*foo)(const posix_spawnattr_t *restrict, sigset_t *restrict) = posix_spawnattr_getsigmask;
int main(void) { return 0; }
