#include <stdio.h>
#ifdef puts
#undef puts
#endif
int (*foo)(const char *) = puts;
int main(void) { return 0; }
