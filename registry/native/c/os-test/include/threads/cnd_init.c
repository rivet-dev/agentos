#include <threads.h>
#ifdef cnd_init
#undef cnd_init
#endif
int (*foo)(cnd_t *) = cnd_init;
int main(void) { return 0; }
