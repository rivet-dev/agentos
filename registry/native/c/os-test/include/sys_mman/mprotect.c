#include <sys/mman.h>
#ifdef mprotect
#undef mprotect
#endif
int (*foo)(void *, size_t, int) = mprotect;
int main(void) { return 0; }
