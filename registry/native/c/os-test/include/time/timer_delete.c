#include <time.h>
#ifdef timer_delete
#undef timer_delete
#endif
int (*foo)(timer_t) = timer_delete;
int main(void) { return 0; }
