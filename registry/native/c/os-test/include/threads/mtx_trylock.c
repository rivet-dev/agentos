#include <threads.h>
#ifdef mtx_trylock
#undef mtx_trylock
#endif
int (*foo)(mtx_t *) = mtx_trylock;
int main(void) { return 0; }
