#include <threads.h>
#ifdef cnd_signal
#undef cnd_signal
#endif
int (*foo)(cnd_t *) = cnd_signal;
int main(void) { return 0; }
