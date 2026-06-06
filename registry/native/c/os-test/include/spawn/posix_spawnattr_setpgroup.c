/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_setpgroup
#undef posix_spawnattr_setpgroup
#endif
int (*foo)(posix_spawnattr_t *, pid_t) = posix_spawnattr_setpgroup;
int main(void) { return 0; }
