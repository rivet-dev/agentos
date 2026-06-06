#include <sys/mman.h>
#ifdef mmap
#undef mmap
#endif
void *(*foo)(void *, size_t, int, int, int, off_t) = mmap;
int main(void) { return 0; }
