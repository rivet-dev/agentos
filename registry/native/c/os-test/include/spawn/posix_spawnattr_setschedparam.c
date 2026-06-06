/*[SPN PS]*/
#include <spawn.h>
#ifdef posix_spawnattr_setschedparam
#undef posix_spawnattr_setschedparam
#endif
int (*foo)(posix_spawnattr_t *restrict, const struct sched_param *restrict) = posix_spawnattr_setschedparam;
int main(void) { return 0; }
