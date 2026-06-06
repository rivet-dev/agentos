#include <stdio.h>
#ifdef vscanf
#undef vscanf
#endif
int (*foo)(const char *restrict, va_list) = vscanf;
int main(void) { return 0; }
