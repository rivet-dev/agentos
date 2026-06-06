#include <stdio.h>
#ifdef getc
#undef getc
#endif
int (*foo)(FILE *) = getc;
int main(void) { return 0; }
