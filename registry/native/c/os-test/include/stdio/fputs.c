#include <stdio.h>
#ifdef fputs
#undef fputs
#endif
int (*foo)(const char *restrict, FILE *restrict) = fputs;
int main(void) { return 0; }
