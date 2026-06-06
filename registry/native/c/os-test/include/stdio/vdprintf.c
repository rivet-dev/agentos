#include <stdio.h>
#ifdef vdprintf
#undef vdprintf
#endif
int (*foo)(int, const char *restrict, va_list) = vdprintf;
int main(void) { return 0; }
