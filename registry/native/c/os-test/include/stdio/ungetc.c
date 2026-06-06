#include <stdio.h>
#ifdef ungetc
#undef ungetc
#endif
int (*foo)(int, FILE *) = ungetc;
int main(void) { return 0; }
