#include <threads.h>
#ifdef cnd_wait
#undef cnd_wait
#endif
int (*foo)(cnd_t *, mtx_t *) = cnd_wait;
int main(void) { return 0; }
