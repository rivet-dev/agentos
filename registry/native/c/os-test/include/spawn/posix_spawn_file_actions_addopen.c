/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_addopen
#undef posix_spawn_file_actions_addopen
#endif
int (*foo)(posix_spawn_file_actions_t *restrict, int, const char *restrict, int, mode_t) = posix_spawn_file_actions_addopen;
int main(void) { return 0; }
