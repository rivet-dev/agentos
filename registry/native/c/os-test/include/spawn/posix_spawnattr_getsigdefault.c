/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_getsigdefault
#undef posix_spawnattr_getsigdefault
#endif
int (*foo)(const posix_spawnattr_t *restrict, sigset_t *restrict) = posix_spawnattr_getsigdefault;
int main(void) { return 0; }
