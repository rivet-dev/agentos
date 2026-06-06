/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_adddup2
#undef posix_spawn_file_actions_adddup2
#endif
int (*foo)(posix_spawn_file_actions_t *, int, int) = posix_spawn_file_actions_adddup2;
int main(void) { return 0; }
