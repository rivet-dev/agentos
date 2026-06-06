/*[SPN PS]*/
#include <spawn.h>
#ifdef posix_spawnattr_setschedpolicy
#undef posix_spawnattr_setschedpolicy
#endif
int (*foo)(posix_spawnattr_t *, int) = posix_spawnattr_setschedpolicy;
int main(void) { return 0; }
