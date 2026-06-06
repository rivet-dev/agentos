#include <stdio.h>
#ifdef fread
#undef fread
#endif
size_t (*foo)(void *restrict, size_t, size_t, FILE *restrict) = fread;
int main(void) { return 0; }
