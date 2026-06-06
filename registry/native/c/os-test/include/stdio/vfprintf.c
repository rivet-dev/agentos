#include <stdio.h>
#ifdef vfprintf
#undef vfprintf
#endif
int (*foo)(FILE *restrict, const char *restrict, va_list) = vfprintf;
int main(void) { return 0; }
