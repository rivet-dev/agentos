/*[MLR]*/
#include <sys/mman.h>
#ifdef munlock
#undef munlock
#endif
int (*foo)(const void *, size_t) = munlock;
int main(void) { return 0; }
