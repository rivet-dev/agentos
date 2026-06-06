#include <threads.h>
#ifdef cnd_broadcast
#undef cnd_broadcast
#endif
int (*foo)(cnd_t *) = cnd_broadcast;
int main(void) { return 0; }
