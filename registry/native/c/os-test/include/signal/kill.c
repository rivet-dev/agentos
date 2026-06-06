#include <signal.h>
#ifdef kill
#undef kill
#endif
int (*foo)(pid_t, int) = kill;
int main(void) { return 0; }
