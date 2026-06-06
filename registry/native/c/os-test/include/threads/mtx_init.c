#include <threads.h>
#ifdef mtx_init
#undef mtx_init
#endif
int (*foo)(mtx_t *, int) = mtx_init;
int main(void) { return 0; }
