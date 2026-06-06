#include <stdio.h>
#ifdef setvbuf
#undef setvbuf
#endif
int (*foo)(FILE *restrict, char *restrict, int, size_t) = setvbuf;
int main(void) { return 0; }
