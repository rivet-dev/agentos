/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_addfchdir
#undef posix_spawn_file_actions_addfchdir
#endif
int (*foo)(posix_spawn_file_actions_t *, int) = posix_spawn_file_actions_addfchdir;
int main(void) { return 0; }
