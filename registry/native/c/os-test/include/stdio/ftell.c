#include <stdio.h>
#ifdef ftell
#undef ftell
#endif
long (*foo)(FILE *) = ftell;
int main(void) { return 0; }
