#include <stdio.h>
#ifdef feof
#undef feof
#endif
int (*foo)(FILE *) = feof;
int main(void) { return 0; }
