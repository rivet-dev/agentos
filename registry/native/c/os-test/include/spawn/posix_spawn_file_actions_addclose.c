/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawn_file_actions_addclose
#undef posix_spawn_file_actions_addclose
#endif
int (*foo)(posix_spawn_file_actions_t *, int) = posix_spawn_file_actions_addclose;
int main(void) { return 0; }
