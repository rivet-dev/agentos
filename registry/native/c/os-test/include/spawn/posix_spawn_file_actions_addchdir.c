/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_addchdir
#undef posix_spawn_file_actions_addchdir
#endif
int (*foo)(posix_spawn_file_actions_t *restrict, const char *restrict) = posix_spawn_file_actions_addchdir;
int main(void) { return 0; }
