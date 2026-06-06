/*[SPN PS]*/
#include <spawn.h>
#ifdef posix_spawnattr_getschedpolicy
#undef posix_spawnattr_getschedpolicy
#endif
int (*foo)(const posix_spawnattr_t *restrict, int *restrict) = posix_spawnattr_getschedpolicy;
int main(void) { return 0; }
