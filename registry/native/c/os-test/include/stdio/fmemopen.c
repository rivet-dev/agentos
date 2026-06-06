#include <stdio.h>
#ifdef fmemopen
#undef fmemopen
#endif
FILE *(*foo)(void *restrict, size_t, const char *restrict) = fmemopen;
int main(void) { return 0; }
