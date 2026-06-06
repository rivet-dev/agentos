/*[ML]*/
#include <sys/mman.h>
#ifdef munlockall
#undef munlockall
#endif
int (*foo)(void) = munlockall;
int main(void) { return 0; }
