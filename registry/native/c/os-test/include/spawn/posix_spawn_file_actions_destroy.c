/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_destroy
#undef posix_spawn_file_actions_destroy
#endif
int (*foo)(posix_spawn_file_actions_t *) = posix_spawn_file_actions_destroy;
int main(void) { return 0; }
