#include <stdio.h>
#ifdef vsprintf
#undef vsprintf
#endif
int (*foo)(char *restrict, const char *restrict, va_list) = vsprintf;
int main(void) { return 0; }
