#include <stdio.h>
#ifdef fseek
#undef fseek
#endif
int (*foo)(FILE *, long, int) = fseek;
int main(void) { return 0; }
