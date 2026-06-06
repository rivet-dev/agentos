/*[SHM]*/
#include <sys/mman.h>
#ifdef shm_unlink
#undef shm_unlink
#endif
int (*foo)(const char *) = shm_unlink;
int main(void) { return 0; }
