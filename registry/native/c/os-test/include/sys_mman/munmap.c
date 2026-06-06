#include <sys/mman.h>
#ifdef munmap
#undef munmap
#endif
int (*foo)(void *, size_t) = munmap;
int main(void) { return 0; }
