#include <time.h>
#ifdef timer_getoverrun
#undef timer_getoverrun
#endif
int (*foo)(timer_t) = timer_getoverrun;
int main(void) { return 0; }
