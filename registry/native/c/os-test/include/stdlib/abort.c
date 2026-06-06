#include <stdlib.h>
#ifdef abort
#undef abort
#endif
 void (*foo)(void) = abort;
int main(void) { return 0; }
