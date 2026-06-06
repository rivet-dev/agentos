/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnattr_destroy
#undef posix_spawnattr_destroy
#endif
int (*foo)(posix_spawnattr_t *) = posix_spawnattr_destroy;
int main(void) { return 0; }
