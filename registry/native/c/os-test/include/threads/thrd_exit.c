#include <threads.h>
#ifdef thrd_exit
#undef thrd_exit
#endif
 void (*foo)(int) = thrd_exit;
int main(void) { return 0; }
