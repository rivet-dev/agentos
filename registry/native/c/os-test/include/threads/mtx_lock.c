#include <threads.h>
#ifdef mtx_lock
#undef mtx_lock
#endif
int (*foo)(mtx_t *) = mtx_lock;
int main(void) { return 0; }
