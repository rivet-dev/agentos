/*[SPN PS]*/
#include <spawn.h>
#ifdef posix_spawnattr_getschedparam
#undef posix_spawnattr_getschedparam
#endif
int (*foo)(const posix_spawnattr_t *restrict, struct sched_param *restrict) = posix_spawnattr_getschedparam;
int main(void) { return 0; }
