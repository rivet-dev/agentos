#include <stdio.h>
#ifdef snprintf
#undef snprintf
#endif
int (*foo)(char *restrict, size_t, const char *restrict, ...) = snprintf;
int main(void) { return 0; }
