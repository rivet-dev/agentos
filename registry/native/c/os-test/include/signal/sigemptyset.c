#include <signal.h>
#ifdef sigemptyset
#undef sigemptyset
#endif
int (*foo)(sigset_t *) = sigemptyset;
int main(void) { return 0; }
