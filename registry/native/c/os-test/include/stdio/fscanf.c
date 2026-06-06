#include <stdio.h>
#ifdef fscanf
#undef fscanf
#endif
int (*foo)(FILE *restrict, const char *restrict, ...) = fscanf;
int main(void) { return 0; }
