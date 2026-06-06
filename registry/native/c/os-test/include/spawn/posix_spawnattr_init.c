/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_init
#undef posix_spawnattr_init
#endif
int (*foo)(posix_spawnattr_t *) = posix_spawnattr_init;
int main(void) { return 0; }
