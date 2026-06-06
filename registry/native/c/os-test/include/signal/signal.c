#include <signal.h>
#ifdef signal
#undef signal
#endif
void (*(*foo)(int, void (*)(int)))(int) = signal;
int main(void) { return 0; }
