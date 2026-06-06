#include <stdio.h>
#ifdef sprintf
#undef sprintf
#endif
int (*foo)(char *restrict, const char *restrict, ...) = sprintf;
int main(void) { return 0; }
