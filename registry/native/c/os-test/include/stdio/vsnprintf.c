#include <stdio.h>
#ifdef vsnprintf
#undef vsnprintf
#endif
int (*foo)(char *restrict, size_t, const char *restrict, va_list) = vsnprintf;
int main(void) { return 0; }
