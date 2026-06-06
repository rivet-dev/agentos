#include <stdio.h>
#ifdef fflush
#undef fflush
#endif
int (*foo)(FILE *) = fflush;
int main(void) { return 0; }
