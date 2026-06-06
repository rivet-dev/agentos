#include <signal.h>
#ifdef sigpending
#undef sigpending
#endif
int (*foo)(sigset_t *) = sigpending;
int main(void) { return 0; }
