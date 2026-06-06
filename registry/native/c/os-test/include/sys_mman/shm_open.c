/*[SHM]*/
#include <sys/mman.h>
#ifdef shm_open
#undef shm_open
#endif
int (*foo)(const char *, int, mode_t) = shm_open;
int main(void) { return 0; }
