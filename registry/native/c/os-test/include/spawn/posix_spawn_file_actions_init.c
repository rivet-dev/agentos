/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_init
#undef posix_spawn_file_actions_init
#endif
int (*foo)(posix_spawn_file_actions_t *) = posix_spawn_file_actions_init;
int main(void) { return 0; }
