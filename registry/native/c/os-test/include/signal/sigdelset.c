#include <signal.h>
#ifdef sigdelset
#undef sigdelset
#endif
int (*foo)(sigset_t *, int) = sigdelset;
int main(void) { return 0; }
