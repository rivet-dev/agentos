#include <stdlib.h>
#ifdef atexit
#undef atexit
#endif
int (*foo)(void (*)(void)) = atexit;
int main(void) { return 0; }
