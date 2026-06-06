#include <signal.h>
#ifdef pthread_sigmask
#undef pthread_sigmask
#endif
int (*foo)(int, const sigset_t *restrict, sigset_t *restrict) = pthread_sigmask;
int main(void) { return 0; }
