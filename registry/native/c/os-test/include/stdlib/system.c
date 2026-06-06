#include <stdlib.h>
#ifdef system
#undef system
#endif
int (*foo)(const char *) = system;
int main(void) { return 0; }
