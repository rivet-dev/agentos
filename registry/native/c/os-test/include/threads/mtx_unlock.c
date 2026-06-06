#include <threads.h>
#ifdef mtx_unlock
#undef mtx_unlock
#endif
int (*foo)(mtx_t *) = mtx_unlock;
int main(void) { return 0; }
