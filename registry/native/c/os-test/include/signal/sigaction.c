#include <signal.h>
#ifdef sigaction
#undef sigaction
#endif
int (*foo)(int, const struct sigaction *restrict, struct sigaction *restrict) = sigaction;
int main(void) { return 0; }
