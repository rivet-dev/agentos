#include <stdio.h>
#ifdef fprintf
#undef fprintf
#endif
int (*foo)(FILE *restrict, const char *restrict, ...) = fprintf;
int main(void) { return 0; }
