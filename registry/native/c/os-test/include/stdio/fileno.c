#include <stdio.h>
#ifdef fileno
#undef fileno
#endif
int (*foo)(FILE *) = fileno;
int main(void) { return 0; }
