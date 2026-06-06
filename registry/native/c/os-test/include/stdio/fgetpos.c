#include <stdio.h>
#ifdef fgetpos
#undef fgetpos
#endif
int (*foo)(FILE *restrict, fpos_t *restrict) = fgetpos;
int main(void) { return 0; }
