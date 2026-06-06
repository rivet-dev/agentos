#include <stdio.h>
#ifdef fgetc
#undef fgetc
#endif
int (*foo)(FILE *) = fgetc;
int main(void) { return 0; }
