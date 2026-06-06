/*[SPN]*/
#include <spawn.h>
#ifdef posix_spawnp
#undef posix_spawnp
#endif
int (*foo)(pid_t *restrict, const char *restrict, const posix_spawn_file_actions_t *restrict, const posix_spawnattr_t *restrict, char *const [restrict], char *const [restrict]) = posix_spawnp;
int main(void) { return 0; }
