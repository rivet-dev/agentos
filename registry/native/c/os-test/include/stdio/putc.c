#include <stdio.h>
#ifdef putc
#undef putc
#endif
int (*foo)(int, FILE *) = putc;
int main(void) { return 0; }
