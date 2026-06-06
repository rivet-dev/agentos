#include <stdio.h>
#ifdef dprintf
#undef dprintf
#endif
int (*foo)(int, const char *restrict, ...) = dprintf;
int main(void) { return 0; }
