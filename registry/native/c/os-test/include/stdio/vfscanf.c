#include <stdio.h>
#ifdef vfscanf
#undef vfscanf
#endif
int (*foo)(FILE *restrict, const char *restrict, va_list) = vfscanf;
int main(void) { return 0; }
