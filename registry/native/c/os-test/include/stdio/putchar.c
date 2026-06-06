#include <stdio.h>
#ifdef putchar
#undef putchar
#endif
int (*foo)(int) = putchar;
int main(void) { return 0; }
