#include <time.h>
#ifdef timer_create
#undef timer_create
#endif
int (*foo)(clockid_t, struct sigevent *restrict, timer_t *restrict) = timer_create;
int main(void) { return 0; }
