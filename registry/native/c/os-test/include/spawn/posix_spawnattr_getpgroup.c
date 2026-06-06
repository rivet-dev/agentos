/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_getpgroup
#undef posix_spawnattr_getpgroup
#endif
int (*foo)(const posix_spawnattr_t *restrict, pid_t *restrict) = posix_spawnattr_getpgroup;
int main(void) { return 0; }
