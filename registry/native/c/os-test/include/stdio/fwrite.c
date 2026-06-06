#include <stdio.h>
#ifdef fwrite
#undef fwrite
#endif
size_t (*foo)(const void *restrict, size_t, size_t, FILE *restrict) = fwrite;
int main(void) { return 0; }
