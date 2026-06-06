/*[MLR]*/
#include <sys/mman.h>
#ifdef mlock
#undef mlock
#endif
int (*foo)(const void *, size_t) = mlock;
int main(void) { return 0; }
