#include <signal.h>
#ifdef sigfillset
#undef sigfillset
#endif
int (*foo)(sigset_t *) = sigfillset;
int main(void) { return 0; }
