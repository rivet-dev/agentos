#include <stdio.h>
#ifdef printf
#undef printf
#endif
int (*foo)(const char *restrict, ...) = printf;
int main(void) { return 0; }
