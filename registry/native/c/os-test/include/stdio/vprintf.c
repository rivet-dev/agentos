#include <stdio.h>
#ifdef vprintf
#undef vprintf
#endif
int (*foo)(const char *restrict, va_list) = vprintf;
int main(void) { return 0; }
