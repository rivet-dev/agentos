#include <signal.h>
#ifdef sigaddset
#undef sigaddset
#endif
int (*foo)(sigset_t *, int) = sigaddset;
int main(void) { return 0; }
