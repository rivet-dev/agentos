#include <stdio.h>
#ifdef setbuf
#undef setbuf
#endif
void (*foo)(FILE *restrict, char *restrict) = setbuf;
int main(void) { return 0; }
