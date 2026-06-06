#include <stdio.h>
#ifdef fputc
#undef fputc
#endif
int (*foo)(int, FILE *) = fputc;
int main(void) { return 0; }
