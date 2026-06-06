#include <signal.h>
#ifdef raise
#undef raise
#endif
int (*foo)(int) = raise;
int main(void) { return 0; }
