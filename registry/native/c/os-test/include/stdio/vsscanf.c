#include <stdio.h>
#ifdef vsscanf
#undef vsscanf
#endif
int (*foo)(const char *restrict, const char *restrict, va_list) = vsscanf;
int main(void) { return 0; }
