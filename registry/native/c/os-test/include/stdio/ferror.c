#include <stdio.h>
#ifdef ferror
#undef ferror
#endif
int (*foo)(FILE *) = ferror;
int main(void) { return 0; }
