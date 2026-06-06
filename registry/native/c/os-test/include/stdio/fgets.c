#include <stdio.h>
#ifdef fgets
#undef fgets
#endif
char *(*foo)(char *restrict, int, FILE *restrict) = fgets;
int main(void) { return 0; }
