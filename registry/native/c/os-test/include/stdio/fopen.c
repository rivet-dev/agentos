#include <stdio.h>
#ifdef fopen
#undef fopen
#endif
FILE *(*foo)(const char *restrict, const char *restrict) = fopen;
int main(void) { return 0; }
