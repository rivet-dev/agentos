/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_setsigdefault
#undef posix_spawnattr_setsigdefault
#endif
int (*foo)(posix_spawnattr_t *restrict, const sigset_t *restrict) = posix_spawnattr_setsigdefault;
int main(void) { return 0; }
